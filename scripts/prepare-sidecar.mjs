// Builds the embral-mcp server in release mode and stages it where Tauri's
// externalBin bundling expects sidecars (`bundle.externalBin` in
// tauri.conf.json). Runs automatically as part of beforeBuildCommand, so
// every `tauri build` ships the MCP server next to the app exe.
import { execSync } from "node:child_process";
import { mkdirSync, copyFileSync } from "node:fs";

execSync("cargo build --release -p embral-mcp", { stdio: "inherit" });
mkdirSync("src-tauri/binaries", { recursive: true });
copyFileSync(
  "target/release/embral-mcp.exe",
  "src-tauri/binaries/embral-mcp-x86_64-pc-windows-msvc.exe",
);
console.log("embral-mcp sidecar staged in src-tauri/binaries/");
