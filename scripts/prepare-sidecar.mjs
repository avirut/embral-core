// Builds the embral-mcp server in release mode and stages it where Tauri's
// externalBin bundling expects sidecars (`bundle.externalBin` in
// tauri.conf.json). Runs automatically as part of beforeBuildCommand, so
// every `tauri build` ships the MCP server next to the app exe.
// Sidecar names carry the host target triple (Tauri resolves
// `binaries/embral-mcp-<triple>[.exe]` per platform).
import { execSync } from "node:child_process";
import { mkdirSync, copyFileSync } from "node:fs";

const triple = execSync("rustc --print host-tuple", { encoding: "utf8" }).trim();
const exe = process.platform === "win32" ? ".exe" : "";

execSync("cargo build --release -p embral-mcp", { stdio: "inherit" });
mkdirSync("src-tauri/binaries", { recursive: true });
copyFileSync(
  `target/release/embral-mcp${exe}`,
  `src-tauri/binaries/embral-mcp-${triple}${exe}`,
);
console.log(`embral-mcp sidecar staged for ${triple}`);
