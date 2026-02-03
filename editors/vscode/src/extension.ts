import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Trace,
} from "vscode-languageclient/node";
import { registerDebugAdapter } from "./debug";
import { getBinaryPath } from "./binary";
import { registerIoPanel } from "./ioPanel";
import { registerLanguageModelTools } from "./lm-tools";
import { augmentDiagnostic } from "./diagnostics";
import {
  registerNamespaceMoveCommand,
  registerNamespaceMoveCodeActions,
  registerNamespaceMoveContext,
} from "./namespaceMove";

let client: LanguageClient | undefined;
let showIecDiagnosticRefs = true;

function sendServerConfig(target: LanguageClient | undefined): void {
  if (!target) {
    return;
  }
  const config = vscode.workspace.getConfiguration("trust-lsp");
  void target.sendNotification("workspace/didChangeConfiguration", {
    settings: { "trust-lsp": config },
  });
}


function resolveServerCommand(context: vscode.ExtensionContext): string {
  return getBinaryPath(context, "trust-lsp", "server.path");
}

function traceFromConfig(value?: string): Trace {
  switch (value) {
    case "messages":
      return Trace.Messages;
    case "verbose":
      return Trace.Verbose;
    default:
      return Trace.Off;
  }
}

function readIecDiagnosticsSetting(config: vscode.WorkspaceConfiguration): boolean {
  return config.get<boolean>("diagnostics.showIecReferences", true);
}

export function activate(context: vscode.ExtensionContext) {
  registerDebugAdapter(context);
  registerIoPanel(context);
  registerLanguageModelTools(context, { getClient: () => client });
  const config = vscode.workspace.getConfiguration("trust-lsp");
  showIecDiagnosticRefs = readIecDiagnosticsSetting(config);
  const command = resolveServerCommand(context);

  const serverOptions: ServerOptions = {
    command,
    args: [],
    options: {
      env: process.env,
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "structured-text" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/*.{st,ST,pou,POU}"
      ),
    },
    middleware: {
      handleDiagnostics(uri, diagnostics, next) {
        next(
          uri,
          diagnostics.map((diagnostic) =>
            augmentDiagnostic(diagnostic, showIecDiagnosticRefs)
          )
        );
      },
    },
  };

  client = new LanguageClient(
    "trust-lsp",
    "Structured Text Language Server",
    serverOptions,
    clientOptions
  );

  context.subscriptions.push(client);
  const startPromise = client.start();
  registerNamespaceMoveCommand(context, client);
  registerNamespaceMoveCodeActions(context);
  registerNamespaceMoveContext(context);

  const trace = traceFromConfig(config.get<string>("trace.server"));
  void startPromise.then(() => {
    if (client) {
      sendServerConfig(client);
      void client.setTrace(trace);
    }
  });

  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("trust-lsp")) {
        sendServerConfig(client);
      }
      if (event.affectsConfiguration("trust-lsp.trace.server")) {
        const updated = vscode.workspace
          .getConfiguration("trust-lsp")
          .get<string>("trace.server");
        if (client) {
          void client.setTrace(traceFromConfig(updated));
        }
      }
      if (event.affectsConfiguration("trust-lsp.server.path")) {
        vscode.window.showInformationMessage(
          "trust-lsp.server.path changed. Reload VS Code to restart the language server."
        );
      }
      if (event.affectsConfiguration("trust-lsp.diagnostics.showIecReferences")) {
        showIecDiagnosticRefs = readIecDiagnosticsSetting(
          vscode.workspace.getConfiguration("trust-lsp")
        );
      }
    })
  );
}

export async function deactivate(): Promise<void> {
  if (!client) {
    return;
  }
  const current = client;
  client = undefined;
  await current.stop();
}