export type RecoveryAction = "login" | "portal" | "retry";
export interface Failure { message: string; action?: RecoveryAction }

export function failureFrom(error: unknown): Failure {
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") {
    const action = "action" in error && ["login", "portal", "retry"].includes(String(error.action))
      ? error.action as RecoveryAction : undefined;
    return { message: error.message, action };
  }
  return { message: String(error) };
}
