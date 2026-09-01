export type StartupStage = "runtime" | "restoring" | "retrying" | "blocked" | "ready";

export interface StartupPresentation {
  title: string;
  detail: string;
  busy: boolean;
}

export function savedProfileCanRestore(
  serverUrl: string,
  accessCode: string,
  accessExpiresAt: number | null,
  nowSeconds = Date.now() / 1_000,
): boolean {
  return serverUrl.trim().length > 0 &&
    /^[A-Za-z0-9]{32}$/.test(accessCode) &&
    accessExpiresAt !== null &&
    Number.isFinite(accessExpiresAt) &&
    accessExpiresAt > nowSeconds;
}

export function startupPresentation(stage: StartupStage): StartupPresentation {
  switch (stage) {
    case "runtime":
      return { title: "Starting Ket", detail: "Preparing the secure client", busy: true };
    case "restoring":
      return { title: "Restoring saved server", detail: "Authorizing this device securely", busy: true };
    case "retrying":
      return {
        title: "Restoring saved server",
        detail: "The server is unavailable. Retrying automatically",
        busy: true,
      };
    case "blocked":
      return {
        title: "Saved server needs attention",
        detail: "Ket could not safely restore this saved server",
        busy: false,
      };
    case "ready":
      return { title: "Ket is ready", detail: "", busy: false };
  }
}
