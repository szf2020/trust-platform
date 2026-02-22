import * as vscode from "vscode";
import * as path from "path";
import { BlocklyEngine, BlocklyWorkspace } from "./blocklyEngine";
import { BlocklyInterpreter } from "./blocklyInterpreter";
import { RuntimeClient, getRuntimeConfig } from "./runtimeClient";

type ExecutionMode = "simulation" | "hardware";

interface ExecutionState {
  workspaceData: BlocklyWorkspace;
  generatedCode: string;
  interpreter?: BlocklyInterpreter;
  mode: ExecutionMode;
  runtimeClient?: RuntimeClient;
}

const WEBVIEW_HTML_TEMPLATE = `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta
      http-equiv="Content-Security-Policy"
      content="default-src 'none'; img-src {{cspSource}} data: https:; style-src {{cspSource}} 'unsafe-inline'; script-src {{cspSource}} 'unsafe-eval' 'unsafe-inline';"
    />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Blockly Editor</title>
    <link rel="stylesheet" href="{{webviewStyle}}" />
    <style>
      * {
        box-sizing: border-box;
        margin: 0;
        padding: 0;
      }

      html,
      body,
      #root {
        width: 100%;
        height: 100%;
        overflow: hidden;
        font-family: var(
          --vscode-font-family,
          -apple-system,
          BlinkMacSystemFont,
          "Segoe UI",
          Roboto,
          Oxygen,
          Ubuntu,
          Cantarell,
          sans-serif
        );
        background-color: var(--vscode-editor-background);
        color: var(--vscode-editor-foreground);
      }
    </style>
  </head>
  <body>
    <div id="root"></div>
    <script src="{{webviewScript}}"></script>
  </body>
</html>
`;

/**
 * Provider for Blockly visual programming editor
 */
export class BlocklyEditorProvider implements vscode.CustomTextEditorProvider {
  private static readonly viewType = "trust-lsp.blockly.editor";
  private activeExecutions = new Map<string, ExecutionState>();

  public static register(context: vscode.ExtensionContext): vscode.Disposable {
    const provider = new BlocklyEditorProvider(context);
    const providerRegistration = vscode.window.registerCustomEditorProvider(
      BlocklyEditorProvider.viewType,
      provider,
      {
        webviewOptions: {
          retainContextWhenHidden: true,
        },
      }
    );
    return providerRegistration;
  }

  constructor(private readonly context: vscode.ExtensionContext) {}

  /**
   * Called when a custom editor is opened
   */
  public async resolveCustomTextEditor(
    document: vscode.TextDocument,
    webviewPanel: vscode.WebviewPanel,
    _token: vscode.CancellationToken
  ): Promise<void> {
    const docId = document.uri.toString();

    // Setup webview
    webviewPanel.webview.options = {
      enableScripts: true,
      localResourceRoots: [
        vscode.Uri.file(path.join(this.context.extensionPath, "media")),
      ],
    };

    webviewPanel.webview.html = this.getHtmlForWebview(webviewPanel.webview);

    // Helper function to update webview
    function updateWebview() {
      void webviewPanel.webview.postMessage({
        type: "update",
        content: document.getText(),
      });
    }

    // Hook up event handlers
    const changeDocumentSubscription = vscode.workspace.onDidChangeTextDocument(
      (e) => {
        if (e.document.uri.toString() === document.uri.toString()) {
          updateWebview();
        }
      }
    );

    // Handle messages from webview
    const messageSubscription = webviewPanel.webview.onDidReceiveMessage((message) => {
      switch (message.type) {
        case "save":
          this.updateTextDocument(document, message.content);
          return;

        case "ready":
          // Send initial content when webview is ready
          updateWebview();
          return;

        case "error":
          void vscode.window.showErrorMessage(
            `Blockly Editor Error: ${message.error}`
          );
          return;

        case "generateCode":
          void this.generateCode(document, webviewPanel);
          return;

        case "startExecution":
          void this.startExecution(
            document,
            webviewPanel,
            message.mode || "simulation"
          );
          return;

        case "stopExecution":
          void this.stopExecution(docId);
          void webviewPanel.webview.postMessage({ type: "executionStopped" });
          return;

        case "executeBlock":
          void this.executeBlock(docId, message.blockId, webviewPanel);
          return;
      }
    });

    // Clean up when editor closes
    webviewPanel.onDidDispose(() => {
      changeDocumentSubscription.dispose();
      messageSubscription.dispose();
      void this.stopExecution(docId, false);
    });

    // Send initial content
    updateWebview();
  }

  /**
   * Get the HTML content for the webview
   */
  private getHtmlForWebview(webview: vscode.Webview): string {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.file(
        path.join(this.context.extensionPath, "media", "blocklyWebview.js")
      )
    );

    const cssUri = webview.asWebviewUri(
      vscode.Uri.file(
        path.join(this.context.extensionPath, "media", "blocklyWebview.css")
      )
    );

    let html = WEBVIEW_HTML_TEMPLATE;

    // Replace placeholders
    html = html.replace(/{{cspSource}}/g, webview.cspSource);
    html = html.replace(/{{webviewScript}}/g, scriptUri.toString());
    html = html.replace(/{{webviewStyle}}/g, cssUri.toString());

    return html;
  }

  /**
   * Update the document with new content from webview
   */
  private updateTextDocument(document: vscode.TextDocument, content: string) {
    const edit = new vscode.WorkspaceEdit();

    // Replace entire document
    edit.replace(
      document.uri,
      new vscode.Range(0, 0, document.lineCount, 0),
      content
    );

    return vscode.workspace.applyEdit(edit);
  }

  /**
   * Generate ST code from Blockly workspace
   */
  private async generateCode(
    document: vscode.TextDocument,
    webviewPanel: vscode.WebviewPanel
  ) {
    try {
      const workspaceData: BlocklyWorkspace = JSON.parse(document.getText());
      const engine = new BlocklyEngine();
      const result = engine.generateCode(workspaceData);

      if (result.errors.length > 0) {
        vscode.window.showWarningMessage(
          `Code generation completed with ${result.errors.length} warnings`
        );
        console.warn("Generation warnings:", result.errors);
      }

      // Send generated code to webview
      void webviewPanel.webview.postMessage({
        type: "codeGenerated",
        code: result.structuredText,
        variables: Array.from(result.variables.entries()),
        errors: result.errors,
      });

      // Optionally save to .st file
      const saveCode = await vscode.window.showQuickPick(
        ["Yes", "No"],
        { placeHolder: "Save generated ST code to file?" }
      );

      if (saveCode === "Yes") {
        const stFileName = document.uri.fsPath.replace(/\.blockly\.json$/, ".st");
        const stFileUri = vscode.Uri.file(stFileName);
        await vscode.workspace.fs.writeFile(
          stFileUri,
          Buffer.from(result.structuredText, "utf8")
        );
        vscode.window.showInformationMessage(`ST code saved to ${path.basename(stFileName)}`);
      }

    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      vscode.window.showErrorMessage(`Code generation failed: ${errorMessage}`);
    }
  }

  /**
   * Start execution of the Blockly program
   */
  private async startExecution(
    document: vscode.TextDocument,
    webviewPanel: vscode.WebviewPanel,
    mode: ExecutionMode
  ) {
    const docId = document.uri.toString();
    
    try {
      // Stop any existing execution
      await this.stopExecution(docId);

      const workspaceData: BlocklyWorkspace = JSON.parse(document.getText());
      const engine = new BlocklyEngine();
      const result = engine.generateCode(workspaceData);

      let runtimeClient: RuntimeClient | undefined;

      // Hardware mode: connect to trust-runtime
      if (mode === "hardware") {
        const config = getRuntimeConfig();
        
        runtimeClient = new RuntimeClient(config);
        
        try {
          await runtimeClient.connect();
          console.log("Blockly: Connected to trust-runtime via Unix socket:", config.controlEndpoint);
          
          // Ensure runtime is in RUN mode (not paused)
          await runtimeClient.resume();
          console.log("Blockly: Runtime resumed (ready for I/O commands)");
          
          vscode.window.showInformationMessage("✅ Connected to trust-runtime for hardware execution");
        } catch (error) {
          vscode.window.showErrorMessage(
            `Failed to connect to trust-runtime: ${error instanceof Error ? error.message : String(error)}`
          );
          return;
        }
      }

      // Create and start interpreter
      const interpreter = new BlocklyInterpreter(workspaceData, mode, runtimeClient);
      
      // Store execution state
      this.activeExecutions.set(docId, {
        workspaceData,
        generatedCode: result.structuredText,
        mode,
        runtimeClient,
        interpreter,
      });

      // Send execution started message
      void webviewPanel.webview.postMessage({
        type: "executionStarted",
        mode,
        code: result.structuredText,
      });

      // Start execution
      void this.executeBlocksWithHighlight(workspaceData, interpreter, webviewPanel);

      vscode.window.showInformationMessage(
        `Blockly program execution started in ${mode} mode`
      );

    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      vscode.window.showErrorMessage(`Failed to start execution: ${errorMessage}`);
    }
  }

  /**
   * Execute blocks with visual highlighting in webview
   */
  private async executeBlocksWithHighlight(
    workspace: BlocklyWorkspace,
    interpreter: BlocklyInterpreter,
    webviewPanel: vscode.WebviewPanel
  ): Promise<void> {
    // Hook into interpreter to send block execution events
    const originalExecute = interpreter['executeBlock'].bind(interpreter);
    
    interpreter['executeBlock'] = async (blockDef: any) => {
      console.log(`[blocklyEditor] Highlighting block: ${blockDef.id}`);
      
      // Highlight block in webview (non-blocking)
      void webviewPanel.webview.postMessage({
        type: "highlightBlock",
        blockId: blockDef.id,
      });

      // Execute the block immediately
      const result = await originalExecute(blockDef);

      // Unhighlight immediately (non-blocking)
      void webviewPanel.webview.postMessage({
        type: "unhighlightBlock",
        blockId: blockDef.id,
      });

      return result;
    };

    // Start execution
    await interpreter.start();
  }

  /**
   * Stop execution of the Blockly program
   */
  private async stopExecution(docId: string, showMessage = true) {
    const execution = this.activeExecutions.get(docId);
    
    if (!execution) {
      return;
    }

    // Stop interpreter
    if (execution.interpreter) {
      execution.interpreter.stop();
    }

    // Disconnect from runtime if connected
    if (execution.runtimeClient) {
      execution.runtimeClient.disconnect();
    }

    this.activeExecutions.delete(docId);

    if (showMessage) {
      vscode.window.showInformationMessage("Blockly program execution stopped");
    }
  }

  /**
   * Execute a specific block (for debugging/step mode)
   */
  private async executeBlock(
    docId: string,
    blockId: string,
    webviewPanel: vscode.WebviewPanel
  ) {
    const execution = this.activeExecutions.get(docId);
    
    if (!execution) {
      vscode.window.showErrorMessage("No active execution");
      return;
    }

    // Send block execution result to webview
    void webviewPanel.webview.postMessage({
      type: "blockExecuted",
      blockId,
    });
  }
}
