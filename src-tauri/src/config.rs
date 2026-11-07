use crate::error::{message, Result};
use crate::vendor::jsonc_document::JsoncDocument;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Format {
    Json,
    Toml,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Field {
    pub path: Vec<String>,
    pub value: Option<Value>,
}

impl Field {
    pub fn new(path: &[&str], value: Option<Value>) -> Self {
        Self {
            path: path.iter().map(|s| s.to_string()).collect(),
            value,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    path: PathBuf,
    format: Format,
    before: Vec<Field>,
    after: Vec<Field>,
    #[serde(default)]
    pending_before: Option<Vec<Field>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Receipt {
    before: Vec<Field>,
    after: Vec<Field>,
    previous_journal: Option<Journal>,
}

pub struct ConfigStore {
    directory: PathBuf,
}

impl ConfigStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn journal_path(&self, app: &str) -> Result<PathBuf> {
        if app.is_empty() || !app.bytes().all(|c| c.is_ascii_lowercase() || c == b'-') {
            return Err(message("无法识别的应用"));
        }
        Ok(self.directory.join(format!("{app}.json")))
    }

    pub fn can_restore(&self, app: &str) -> bool {
        self.journal_path(app).is_ok_and(|p| p.exists())
    }

    fn lock(&self) -> Result<File> {
        private_directory(&self.directory)?;
        let file = private_open(&self.directory.join("config.lock"))?;
        file.try_lock_exclusive()
            .map_err(|_| message("另一项配置操作正在进行，请稍后重试"))?;
        Ok(file)
    }

    fn load(&self, app: &str) -> Result<Option<Journal>> {
        let path = self.journal_path(app)?;
        if path.exists() {
            Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
        } else {
            Ok(None)
        }
    }

    /// Every journal is written BEFORE the client config. A crash therefore leaves
    /// a recoverable intent, never a client secret with no matching backup.
    pub fn apply(
        &self,
        app: &str,
        path: &Path,
        format: Format,
        fields: Vec<Field>,
    ) -> Result<Receipt> {
        let _lock = self.lock()?;
        self.apply_unlocked(app, path, format, fields, false)
    }

    fn apply_unlocked(
        &self,
        app: &str,
        path: &Path,
        format: Format,
        fields: Vec<Field>,
        restoring: bool,
    ) -> Result<Receipt> {
        let source = read_source(path, format)?;
        let current = parse(&source, format)?;
        let previous = self.load(app)?;
        if let Some(journal) = &previous {
            if journal.path != path {
                return Err(message("配置目录已改变，请先恢复原目录中的修改"));
            }
            // Also accepts an interrupted write where the old state is still on disk.
            if !restoring && !journal_matches(&current, journal) {
                return Err(message(
                    "接入配置已被其他程序修改，请先检查配置，避免覆盖你的修改",
                ));
            }
        }
        let before = capture(&current, &fields);
        let output = patched(&source, format, &fields)?;
        // Keep checking fields touched by earlier modes, even when this operation
        // does not modify them. Restore must not overwrite a later user edit.
        let mut after = fields.clone();
        if let Some(journal) = &previous {
            for field in &journal.after {
                if !after.iter().any(|f| f.path == field.path) {
                    after.push(Field {
                        path: field.path.clone(),
                        value: at(&current, &field.path).cloned(),
                    });
                }
            }
        }
        let mut original = previous
            .as_ref()
            .map(|j| j.before.clone())
            .unwrap_or_default();
        for field in &before {
            if !original.iter().any(|f| f.path == field.path) {
                original.push(field.clone());
            }
        }
        let mut journal = Journal {
            path: path.to_path_buf(),
            format,
            before: original,
            pending_before: Some(capture(&current, &after)),
            after,
        };
        atomic_write(
            &self.journal_path(app)?,
            &serde_json::to_vec_pretty(&journal)?,
        )?;
        if let Err(error) = compare_and_write(path, &source, &output) {
            self.save_journal(app, previous.as_ref())?;
            return Err(error);
        }
        journal.pending_before = None;
        // If finalizing the journal fails, the pending intent still supports recovery.
        let _ = self.save_journal(app, Some(&journal));
        Ok(Receipt {
            before,
            after: fields,
            previous_journal: previous,
        })
    }

    pub fn rollback(&self, app: &str, path: &Path, format: Format, receipt: Receipt) -> Result<()> {
        let _lock = self.lock()?;
        let source = read_source(path, format)?;
        if !matches(&parse(&source, format)?, &receipt.after) {
            return Err(message(
                "验证失败后检测到配置被修改，已保留备份，请先恢复配置",
            ));
        }
        compare_and_write(path, &source, &patched(&source, format, &receipt.before)?)?;
        self.save_journal(app, receipt.previous_journal.as_ref())
    }

    pub fn restore(&self, app: &str, expected_path: &Path) -> Result<()> {
        let _lock = self.lock()?;
        let journal = self
            .load(app)?
            .ok_or_else(|| message("没有需要恢复的配置"))?;
        if journal.path != expected_path {
            return Err(message("配置目录已改变，无法自动恢复"));
        }
        let source = read_source(&journal.path, journal.format)?;
        compare_and_write(
            &journal.path,
            &source,
            &patched(&source, journal.format, &journal.before)?,
        )?;
        self.save_journal(app, None)
    }

    // Hold the file lock through verification. The complete rollback intent is
    // persisted before the first file changes, including multi-file desktop configs.
    pub fn begin(&self, app: &str, changes: Vec<Change>) -> Result<Batch> {
        self.begin_internal(app, changes, false)
    }

    /// Explicit restore replaces all originally managed fields, even if the
    /// client or user has since changed them. Other settings remain untouched.
    pub fn begin_restore(&self, app: &str, changes: Vec<Change>) -> Result<Batch> {
        self.begin_internal(app, changes, true)
    }

    fn begin_internal(&self, app: &str, changes: Vec<Change>, restoring: bool) -> Result<Batch> {
        let lock = self.lock()?;
        self.recover_unlocked()?;
        let mut entries = Vec::new();
        for change in &changes {
            self.journal_path(&change.id)?;
            let source = read_source(&change.path, change.format)?;
            let current = parse(&source, change.format)?;
            let previous = self.load(&change.id)?;
            if previous.as_ref().is_some_and(|j| {
                j.path != change.path || (!restoring && !journal_matches(&current, j))
            }) {
                return Err(message("接入配置已被其他程序修改，原备份已保留"));
            }
            // Preflight every parse and patch before mutating any file.
            patched(&source, change.format, &change.fields)?;
            entries.push(BatchEntry {
                id: change.id.clone(),
                path: change.path.clone(),
                format: change.format,
                receipt: Receipt {
                    before: capture(&current, &change.fields),
                    after: change.fields.clone(),
                    previous_journal: previous,
                },
            });
        }
        self.journal_path(app)?;
        let intent = self.directory.join("pending-transaction.json");
        atomic_write(&intent, &serde_json::to_vec(&entries)?)?;
        for change in changes {
            if let Err(e) = self.apply_unlocked(
                &change.id,
                &change.path,
                change.format,
                change.fields,
                restoring,
            ) {
                self.recover_unlocked()?;
                return Err(e);
            }
        }
        Ok(Batch {
            _lock: lock,
            intent,
            entries,
        })
    }

    pub fn commit(&self, batch: Batch) -> Result<()> {
        std::fs::remove_file(batch.intent)?;
        Ok(())
    }

    pub fn revert(&self, _batch: Batch) -> Result<()> {
        self.recover_unlocked()
    }

    pub fn recover(&self) -> Result<()> {
        let _lock = self.lock()?;
        self.recover_unlocked()
    }

    fn recover_unlocked(&self) -> Result<()> {
        let path = self.directory.join("pending-transaction.json");
        if !path.exists() {
            return Ok(());
        }
        let entries: Vec<BatchEntry> = serde_json::from_slice(&fs::read(&path)?)?;
        // Check all files first so a conflict cannot cause a partial restore.
        for entry in &entries {
            let current = read_value(&entry.path, entry.format)?;
            if !matches(&current, &entry.receipt.after) && !matches(&current, &entry.receipt.before)
            {
                return Err(message(
                    "中断操作的配置已被修改，无法自动恢复；原备份已保留",
                ));
            }
        }
        for entry in entries.iter().rev() {
            let source = read_source(&entry.path, entry.format)?;
            compare_and_write(
                &entry.path,
                &source,
                &patched(&source, entry.format, &entry.receipt.before)?,
            )?;
            self.save_journal(&entry.id, entry.receipt.previous_journal.as_ref())?;
        }
        fs::remove_file(path)?;
        Ok(())
    }

    pub fn restore_changes(&self, targets: Vec<(String, PathBuf)>) -> Result<Vec<Change>> {
        let mut changes = Vec::new();
        for (id, path) in targets {
            if let Some(journal) = self.load(&id)? {
                if journal.path != path {
                    return Err(message("配置目录已改变，无法自动恢复"));
                }
                changes.push(Change {
                    id,
                    path,
                    format: journal.format,
                    fields: journal.before,
                });
            }
        }
        if changes.is_empty() {
            return Err(message("没有需要恢复的配置"));
        }
        Ok(changes)
    }

    pub fn finish_restore(&self, batch: Batch) -> Result<()> {
        for entry in &batch.entries {
            self.save_journal(&entry.id, None)?;
        }
        self.commit(batch)
    }

    fn save_journal(&self, app: &str, journal: Option<&Journal>) -> Result<()> {
        let path = self.journal_path(app)?;
        if let Some(journal) = journal {
            atomic_write(&path, &serde_json::to_vec_pretty(journal)?)?;
        } else if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

pub fn read_value(path: &Path, format: Format) -> Result<Value> {
    parse(&read_source(path, format)?, format)
}

fn read_source(path: &Path, format: Format) -> Result<String> {
    reject_symlink(path)?;
    match fs::read_to_string(path) {
        Ok(source) => Ok(source),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(match format {
            Format::Json => "{}\n",
            Format::Toml => "",
        }
        .into()),
        Err(e) => Err(e.into()),
    }
}

fn parse(source: &str, format: Format) -> Result<Value> {
    match format {
        Format::Json => Ok(JsoncDocument::parse(source)?.value().clone()),
        Format::Toml => {
            let doc = source
                .parse::<DocumentMut>()
                .map_err(|_| message("现有 TOML 配置无法解析，请先修复；未修改文件"))?;
            toml_edit::de::from_str::<Value>(&doc.to_string())
                .map_err(|_| message("现有 TOML 配置无法解析"))
        }
    }
}

fn capture(current: &Value, fields: &[Field]) -> Vec<Field> {
    fields
        .iter()
        .map(|f| Field {
            path: f.path.clone(),
            value: at(current, &f.path).cloned(),
        })
        .collect()
}

fn at<'a>(mut value: &'a Value, path: &[String]) -> Option<&'a Value> {
    for key in path {
        value = value.get(key)?;
    }
    Some(value)
}

fn matches(current: &Value, fields: &[Field]) -> bool {
    fields
        .iter()
        .all(|f| at(current, &f.path) == f.value.as_ref())
}

fn journal_matches(current: &Value, journal: &Journal) -> bool {
    matches(current, &journal.after)
        || matches(current, &journal.before)
        || journal
            .pending_before
            .as_ref()
            .is_some_and(|fields| matches(current, fields))
}

fn patched(source: &str, format: Format, fields: &[Field]) -> Result<String> {
    match format {
        Format::Json => {
            let mut document = JsoncDocument::parse(source)?;
            let mut desired = document.value().clone();
            for field in fields {
                patch_json(&mut desired, &field.path, field.value.as_ref())?;
            }
            document.apply(&desired)?;
            document.validated_source()
        }
        Format::Toml => {
            let mut document = source
                .parse::<DocumentMut>()
                .map_err(|_| message("现有 TOML 配置无法解析"))?;
            for field in fields {
                patch_toml(document.as_item_mut(), &field.path, field.value.as_ref())?;
            }
            Ok(document.to_string())
        }
    }
}

fn patch_json(root: &mut Value, path: &[String], desired: Option<&Value>) -> Result<()> {
    let (key, parents) = path.split_last().ok_or_else(|| message("无效的配置字段"))?;
    let mut value = root;
    for parent in parents {
        let map = value
            .as_object_mut()
            .ok_or_else(|| message("配置字段类型不匹配，未修改文件"))?;
        if !map.contains_key(parent) && desired.is_none() {
            return Ok(());
        }
        value = map
            .entry(parent)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let map = value
        .as_object_mut()
        .ok_or_else(|| message("配置字段类型不匹配，未修改文件"))?;
    match desired {
        Some(v) => {
            map.insert(key.clone(), v.clone());
        }
        None => {
            map.remove(key);
        }
    }
    Ok(())
}

fn patch_toml(item: &mut Item, path: &[String], desired: Option<&Value>) -> Result<()> {
    let (key, parents) = path.split_last().ok_or_else(|| message("无效的配置字段"))?;
    let mut parent = item;
    for part in parents {
        let table = parent
            .as_table_like_mut()
            .ok_or_else(|| message("TOML 字段类型不匹配，未修改文件"))?;
        if !table.contains_key(part) {
            if desired.is_none() {
                return Ok(());
            }
            table.insert(part, Item::Table(toml_edit::Table::new()));
        }
        parent = table.get_mut(part).unwrap();
    }
    let table = parent
        .as_table_like_mut()
        .ok_or_else(|| message("TOML 字段类型不匹配，未修改文件"))?;
    if let Some(value) = desired {
        let wrapper = serde_json::json!({ "entry": value });
        let converted =
            toml_edit::ser::to_document(&wrapper).map_err(|_| message("无法生成 TOML 配置"))?;
        let mut replacement = converted.get("entry").unwrap().clone();
        if let (Some(old), Some(new)) = (
            table.get(key).and_then(Item::as_value),
            replacement.as_value_mut(),
        ) {
            *new.decor_mut() = old.decor().clone();
        }
        if let Some(slot) = table.get_mut(key) {
            *slot = replacement;
        } else {
            table.insert(key, replacement);
        }
    } else {
        table.remove(key);
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(message("配置文件是符号链接，请先确认真实位置；未修改文件"));
    }
    Ok(())
}

pub fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn private_open(path: &Path) -> Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    reject_symlink(path)?;
    let parent = path.parent().ok_or_else(|| message("无效的配置路径"))?;
    private_directory(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn compare_and_write(path: &Path, before: &str, after: &str) -> Result<()> {
    let current = fs::read_to_string(path).unwrap_or_default();
    // Missing JSON files use an empty-object source.
    if current != before && !(current.is_empty() && !path.exists() && before == "{}\n") {
        return Err(message("配置文件在操作中发生变化，请重试；未覆盖修改"));
    }
    atomic_write(path, after.as_bytes())
}

pub struct Change {
    pub id: String,
    pub path: PathBuf,
    pub format: Format,
    pub fields: Vec<Field>,
}
#[derive(Serialize, Deserialize)]
struct BatchEntry {
    id: String,
    path: PathBuf,
    format: Format,
    receipt: Receipt,
}
pub struct Batch {
    _lock: File,
    intent: PathBuf,
    entries: Vec<BatchEntry>,
}
