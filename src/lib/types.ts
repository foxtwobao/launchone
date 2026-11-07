export type Mode = "tokenone" | "official";

export interface AppManifest {
  id: string;
  name: string;
  description: string;
  command: string;
  icon: string;
  supportsOfficial: boolean;
  platforms: string[];
}

export interface AppStatus {
  manifest: AppManifest;
  installed: boolean;
  tokenoneSupported: boolean;
  version: string | null;
  mode: Mode | null;
  canRestore: boolean;
  supported: boolean;
  error: string | null;
}

export interface Account { id: string; name: string }
export interface Snapshot {
  apps: AppStatus[];
  account: Account | null;
  loginReady: boolean;
  tokenoneReady: boolean;
  desktop: boolean;
}

export interface Progress { appId: string; stage: string; message: string }

export interface Bridge {
  snapshot(): Promise<Snapshot>;
  login(): Promise<Account>;
  cancelLogin(): Promise<void>;
  logout(): Promise<void>;
  enable(appId: string): Promise<void>;
  official(appId: string): Promise<void>;
  restore(appId: string): Promise<void>;
  open(appId: string): Promise<void>;
  install(appId: string): Promise<boolean>;
  defaults(): Promise<{ appId: string; message: string }[]>;
  subscribe(callback: (progress: Progress) => void): Promise<() => void>;
}
