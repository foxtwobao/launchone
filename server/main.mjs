import { createApp, readConfig } from "./app.mjs";

try {
  const config = readConfig();
  const server = createApp(config).listen(config.port, config.host, () => console.log("LaunchOne credential service is listening"));
  server.requestTimeout = 60_000;
  server.headersTimeout = 15_000;
  server.on("error", () => { console.error("LaunchOne service could not start"); process.exitCode = 1; });
} catch {
  console.error("LaunchOne service configuration is invalid; see server/.env.example");
  process.exitCode = 1;
}
