import * as path from "path";
import * as fs from "fs";
import * as os from "os";
import { execSync } from "child_process";
import { runTests } from "@vscode/test-electron";

async function main(): Promise<void> {
  const extensionDevelopmentPath = path.resolve(__dirname, "../../");
  const extensionTestsPath = path.resolve(__dirname, "./suite/index");
  const repoRoot = path.resolve(extensionDevelopmentPath, "..", "..");
  const workspacePath = fs.mkdtempSync(
    path.join(os.tmpdir(), "trust-lsp-vscode-workspace-")
  );

  const defaultServerName =
    process.platform === "win32" ? "trust-lsp.exe" : "trust-lsp";
  const defaultServerPath = path.join(
    repoRoot,
    "target",
    "debug",
    defaultServerName
  );
  const configured = process.env.ST_LSP_TEST_SERVER?.trim();
  const serverPath =
    configured && fs.existsSync(configured) ? configured : defaultServerPath;

  if (!configured) {
    execSync("cargo build -p trust-lsp", {
      cwd: repoRoot,
      stdio: "inherit",
    });
  } else if (!fs.existsSync(serverPath)) {
    throw new Error(`ST_LSP_TEST_SERVER not found at ${serverPath}`);
  }

  await runTests({
    extensionDevelopmentPath,
    extensionTestsPath,
    launchArgs: [workspacePath],
    extensionTestsEnv: {
      ST_LSP_TEST_SERVER: serverPath,
    },
  });
}

main().catch((error) => {
  console.error("Failed to run VS Code extension tests");
  console.error(error);
  process.exit(1);
});
