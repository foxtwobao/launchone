import { useEffect, useRef } from "react";

export function ReopenPrompt({ names, onConfirm, onLater }: { names: string[]; onConfirm(): void; onLater(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);
  return <dialog ref={dialog} className="reopen-dialog" aria-labelledby="reopen-title" aria-describedby="reopen-description" onCancel={event => { event.preventDefault(); onLater(); }}>
    <h2 id="reopen-title">重新打开，使配置生效</h2>
    <p id="reopen-description">配置已更新。确认后将正常退出并重新打开 {names.join("、")}。请先保存正在进行的工作。</p>
    <p>应用未能退出时会提示你处理，不会强制结束任务。</p>
    <div className="dialog-actions"><button className="setup-button" autoFocus onClick={onLater}>稍后</button><button className="primary-button" onClick={onConfirm}>重新打开</button></div>
  </dialog>;
}
