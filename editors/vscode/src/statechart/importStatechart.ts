import * as vscode from "vscode";

type ImportStatechartArgs = {
  sourceUri?: vscode.Uri | string;
  targetUri?: vscode.Uri | string;
  overwrite?: boolean;
  openAfterImport?: boolean;
};

export const IMPORT_STATECHART_COMMAND = "trust-lsp.statechart.import";

function asUri(value?: vscode.Uri | string): vscode.Uri | undefined {
  if (!value) {
    return undefined;
  }
  if (value instanceof vscode.Uri) {
    return value;
  }
  try {
    if (value.includes("://")) {
      return vscode.Uri.parse(value);
    }
    return vscode.Uri.file(value);
  } catch {
    return undefined;
  }
}

async function pathExists(uri: vscode.Uri): Promise<boolean> {
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}

async function isValidStatechart(uri: vscode.Uri): Promise<boolean> {
  try {
    const content = await vscode.workspace.fs.readFile(uri);
    const text = Buffer.from(content).toString("utf8");
    const json = JSON.parse(text);
    // Basic validation: must have id and states
    return (
      typeof json === "object" &&
      json !== null &&
      "id" in json &&
      "states" in json
    );
  } catch {
    return false;
  }
}

async function promptForSourceFile(): Promise<vscode.Uri | undefined> {
  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri;
  
  // Try to find examples/statecharts folder relative to workspace
  let defaultUri = workspaceRoot;
  if (workspaceRoot) {
    const examplesPath = vscode.Uri.joinPath(workspaceRoot, "examples/statecharts");
    try {
      await vscode.workspace.fs.stat(examplesPath);
      defaultUri = examplesPath;
    } catch {
      // If examples folder doesn't exist, use workspace root
    }
  }
  
  const selected = await vscode.window.showOpenDialog({
    canSelectFiles: true,
    canSelectFolders: false,
    canSelectMany: false,
    defaultUri: defaultUri,
    filters: {
      "Statechart Files": ["json"],
      "All Files": ["*"],
    },
    openLabel: "Select Statechart to Import",
  });
  return selected?.[0];
}

async function promptForTargetFolder(): Promise<vscode.Uri | undefined> {
  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri;
  if (workspaceRoot) {
    return workspaceRoot;
  }
  
  const selected = await vscode.window.showOpenDialog({
    canSelectFiles: false,
    canSelectFolders: true,
    canSelectMany: false,
    openLabel: "Select Destination Folder",
  });
  return selected?.[0];
}

async function confirmOverwrite(targetUri: vscode.Uri): Promise<boolean> {
  const selection = await vscode.window.showWarningMessage(
    `The file already exists: ${targetUri.fsPath}\nDo you want to overwrite it?`,
    { modal: true },
    "Overwrite",
    "Cancel"
  );
  return selection === "Overwrite";
}

async function copyStatechart(
  sourceUri: vscode.Uri,
  targetUri: vscode.Uri
): Promise<void> {
  const content = await vscode.workspace.fs.readFile(sourceUri);
  await vscode.workspace.fs.writeFile(targetUri, content);
}

async function resolveSourceAndTarget(
  args?: ImportStatechartArgs
): Promise<{ source: vscode.Uri; target: vscode.Uri } | undefined> {
  // Resolve source file
  const sourceUri = asUri(args?.sourceUri) ?? (await promptForSourceFile());
  if (!sourceUri) {
    return undefined;
  }

  const sourceExists = await pathExists(sourceUri);
  if (!sourceExists) {
    vscode.window.showErrorMessage(`Source file not found: ${sourceUri.fsPath}`);
    return undefined;
  }

  const isValid = await isValidStatechart(sourceUri);
  if (!isValid) {
    vscode.window.showErrorMessage(
      `Invalid statechart file. Must be a JSON file with 'id' and 'states' properties.`
    );
    return undefined;
  }

  // Resolve target location
  let targetUri: vscode.Uri | undefined = asUri(args?.targetUri);
  
  if (!targetUri) {
    const targetFolder = await promptForTargetFolder();
    if (!targetFolder) {
      return undefined;
    }

    // Extract filename from source
    const fileName =
      sourceUri.path.split("/").pop() ?? "imported.statechart.json";
    targetUri = vscode.Uri.joinPath(targetFolder, fileName);
  }

  return { source: sourceUri, target: targetUri };
}

export function registerImportStatechartCommand(
  context: vscode.ExtensionContext
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand(
      IMPORT_STATECHART_COMMAND,
      async (args?: ImportStatechartArgs) => {
        const resolved = await resolveSourceAndTarget(args);
        if (!resolved) {
          return;
        }

        const { source, target } = resolved;
        const exists = await pathExists(target);
        
        if (exists) {
          const shouldOverwrite =
            args?.overwrite ?? (await confirmOverwrite(target));
          if (!shouldOverwrite) {
            return;
          }
        }

        try {
          await copyStatechart(source, target);

          const openAfter = args?.openAfterImport ?? true;
          if (openAfter) {
            const doc = await vscode.workspace.openTextDocument(target);
            await vscode.window.showTextDocument(doc);
          }

          vscode.window.showInformationMessage(
            `Statechart imported successfully: ${target.fsPath}`
          );
        } catch (error) {
          vscode.window.showErrorMessage(
            `Failed to import statechart: ${error instanceof Error ? error.message : String(error)}`
          );
        }
      }
    )
  );
}
