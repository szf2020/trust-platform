import React, { useCallback, useEffect, useState } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  Panel,
  BackgroundVariant,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { StateNode } from "./StateNode";
import { PropertiesPanel } from "./PropertiesPanel";
import { ExecutionPanel } from "./ExecutionPanel";
import { ActionMappingsPanel } from "./ActionMappingsPanel";
import { useStateChart } from "./hooks/useStateChart";
import {
  VSCodeAPI,
  WebviewToExtensionMessage,
  ExtensionToWebviewMessage,
  StateChartNode,
  StateChartEdge,
  ExecutionState,
} from "./types";

// VSCode API for webview communication
declare const acquireVsCodeApi: () => VSCodeAPI;
const vscode = acquireVsCodeApi();

const nodeTypes = {
  stateNode: StateNode,
} as any; // Type assertion to avoid @xyflow/react type inference issues

/**
 * Main StateChart Editor Component
 */
export const StateChartEditor: React.FC = () => {
  const {
    nodes,
    edges,
    actionMappings,
    onNodesChange,
    onEdgesChange,
    onConnect,
    addNewState,
    updateNodeData,
    updateEdgeData,
    updateActionMappings,
    deleteSelected,
    autoLayout,
    exportToXState,
    importFromXState,
    setNodes,
  } = useStateChart();

  const [selectedNode, setSelectedNode] = useState<StateChartNode | null>(null);
  const [selectedEdge, setSelectedEdge] = useState<StateChartEdge | null>(null);
  const [executionState, setExecutionState] = useState<ExecutionState | null>(null);
  const [isRunning, setIsRunning] = useState(false);

  // Handle messages from extension
  useEffect(() => {
    const handleMessage = (event: MessageEvent<ExtensionToWebviewMessage>) => {
      const message = event.data;

      switch (message.type) {
        case "init":
        case "update":
          try {
            if (message.content) {
              const config = JSON.parse(message.content);
              importFromXState(config);
            }
          } catch (error) {
            console.error("Failed to parse StateChart config:", error);
            vscode.postMessage({
              type: "error",
              error: String(error),
            } as WebviewToExtensionMessage);
          }
          break;

        case "executionState":
          setExecutionState(message.state);
          setIsRunning(true);
          // Update active state indicator
          updateActiveState(message.state.currentState);
          break;

        case "executionStopped":
          setExecutionState(null);
          setIsRunning(false);
          // Clear active state indicators
          updateActiveState(null);
          break;
      }
    };

    window.addEventListener("message", handleMessage);
    
    // Notify extension that webview is ready
    vscode.postMessage({ type: "ready" } as WebviewToExtensionMessage);

    return () => window.removeEventListener("message", handleMessage);
  }, [importFromXState]);

  // Update active state indicator on nodes
  const updateActiveState = useCallback(
    (activeStateName: string | null) => {
      setNodes((nds) =>
        nds.map((node) => ({
          ...node,
          data: {
            ...node.data,
            isActive: node.data.label === activeStateName,
          },
        }))
      );
    },
    [setNodes]
  );

  // Save changes to document
  const handleSave = useCallback(() => {
    const config = exportToXState();
    const content = JSON.stringify(config, null, 2);
    vscode.postMessage({
      type: "save",
      content,
    } as WebviewToExtensionMessage);
  }, [exportToXState]);

  // Execution control handlers
  const handleStartExecution = useCallback((mode: import("./types").ExecutionMode) => {
    vscode.postMessage({ 
      type: "startExecution",
      mode,
    } as WebviewToExtensionMessage);
  }, []);

  const handleStopExecution = useCallback(() => {
    vscode.postMessage({ type: "stopExecution" } as WebviewToExtensionMessage);
  }, []);

  const handleSendEvent = useCallback((event: string) => {
    vscode.postMessage({
      type: "sendEvent",
      event,
    } as WebviewToExtensionMessage);
  }, []);

  // Handle selection changes
  const handleSelectionChange = useCallback(
    ({ nodes: selectedNodes, edges: selectedEdges }: any) => {
      setSelectedNode(selectedNodes[0] || null);
      setSelectedEdge(selectedEdges[0] || null);
    },
    []
  );

  // Toolbar actions
  const handleAddState = useCallback(() => {
    addNewState("normal");
  }, [addNewState]);

  const handleAddInitialState = useCallback(() => {
    addNewState("initial");
  }, [addNewState]);

  const handleDelete = useCallback(() => {
    deleteSelected();
    setSelectedNode(null);
    setSelectedEdge(null);
  }, [deleteSelected]);

  return (
    <div style={{ width: "100%", height: "100vh", display: "flex" }}>
      {/* Main editor area */}
      <div style={{ flex: 1, position: "relative" }}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onSelectionChange={handleSelectionChange}
          nodeTypes={nodeTypes}
          fitView
          snapToGrid
          snapGrid={[15, 15]}
          defaultEdgeOptions={{
            type: "smoothstep",
            animated: true,
            style: {
              stroke: "var(--vscode-editorWidget-border)",
              strokeWidth: 2,
            },
          }}
          style={{
            background: "var(--vscode-editor-background)",
          }}
        >
          <Background
            variant={BackgroundVariant.Dots}
            gap={20}
            size={1}
            color="var(--vscode-editorWidget-border)"
          />
          <Controls />
          <MiniMap
            nodeColor={(node) => {
              const data = node.data as any;
              switch (data?.type) {
                case "initial":
                  return "#4caf50";
                case "final":
                  return "#f44336";
                case "compound":
                  return "#2196f3";
                default:
                  return "#757575";
              }
            }}
            style={{
              backgroundColor: "var(--vscode-editor-background)",
              border: "1px solid var(--vscode-panel-border)",
            }}
          />

          {/* Toolbar Panel */}
          <Panel
            position="top-left"
            style={{
              display: "flex",
              gap: "8px",
              padding: "8px",
              backgroundColor: "var(--vscode-editor-background)",
              border: "1px solid var(--vscode-panel-border)",
              borderRadius: "4px",
            }}
          >
            <button
              onClick={handleAddState}
              style={buttonStyle}
              title="Add Normal State"
            >
              ➕ State
            </button>
            <button
              onClick={handleAddInitialState}
              style={buttonStyle}
              title="Add Initial State"
            >
              🟢 Initial
            </button>
            <button
              onClick={() => addNewState("final")}
              style={buttonStyle}
              title="Add Final State"
            >
              🔴 Final
            </button>
            <div style={{ width: "1px", background: "var(--vscode-panel-border)" }} />
            <button
              onClick={handleDelete}
              style={buttonStyle}
              title="Delete Selected"
              disabled={!selectedNode && !selectedEdge}
            >
              🗑️ Delete
            </button>
            <button onClick={autoLayout} style={buttonStyle} title="Auto Layout">
              🔀 Layout
            </button>
            <div style={{ width: "1px", background: "var(--vscode-panel-border)" }} />
            <button onClick={handleSave} style={buttonStyle} title="Save">
              💾 Save
            </button>
          </Panel>

          {/* Info Panel */}
          <Panel
            position="bottom-right"
            style={{
              padding: "8px 12px",
              backgroundColor: "var(--vscode-editor-background)",
              border: "1px solid var(--vscode-panel-border)",
              borderRadius: "4px",
              fontSize: "12px",
            }}
          >
            <div>Nodes: {nodes.length}</div>
            <div>Transitions: {edges.length}</div>
            {selectedNode && <div>Selected: {selectedNode.data.label}</div>}
          </Panel>
        </ReactFlow>
      </div>

      {/* Properties Panel (Sidebar) */}
      <div
        style={{
          width: "320px",
          borderLeft: "1px solid var(--vscode-panel-border)",
          backgroundColor: "var(--vscode-sideBar-background)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        {/* Execution Panel */}
        <ExecutionPanel
          executionState={executionState}
          isRunning={isRunning}
          onStart={handleStartExecution}
          onStop={handleStopExecution}
          onSendEvent={handleSendEvent}
        />

        {/* Properties Panel */}
        <PropertiesPanel
          selectedNode={selectedNode}
          selectedEdge={selectedEdge}
          onUpdateNode={updateNodeData}
          onUpdateEdge={updateEdgeData}
        />
        {/* Action Mappings Panel */}
        <ActionMappingsPanel
          actionMappings={actionMappings}
          nodes={nodes}
          onUpdateActionMappings={updateActionMappings}
        />
      </div>
    </div>
  );
};

// Button style matching VSCode theme
const buttonStyle: React.CSSProperties = {
  padding: "6px 12px",
  fontSize: "13px",
  backgroundColor: "var(--vscode-button-background)",
  color: "var(--vscode-button-foreground)",
  border: "1px solid var(--vscode-button-border)",
  borderRadius: "4px",
  cursor: "pointer",
  fontFamily: "var(--vscode-font-family)",
  transition: "all 0.2s",
};
