import type { Failure } from "../lib/failure";

interface Props {
  failure: Failure;
  disabled: boolean;
  portalReady: boolean;
  onRetry(): void;
  onLogin(): void;
  onPortal(): void;
}

export function RecoveryActions({ failure, disabled, portalReady, onRetry, onLogin, onPortal }: Props) {
  return <div className="recovery-actions">
    {failure.action === "login" ? <button className="text-button" disabled={disabled} onClick={onLogin}>重新登录</button> : <>
      {failure.action === "portal" && portalReady && <button className="text-button" disabled={disabled} onClick={onPortal}>前往 TokenOne</button>}
      <button className="text-button" disabled={disabled} onClick={onRetry}>重试</button>
    </>}
    {failure.action === "portal" && portalReady && <span>完成后返回此处重试</span>}
  </div>;
}
