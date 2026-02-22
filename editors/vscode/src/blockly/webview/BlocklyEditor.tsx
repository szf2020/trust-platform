import React, { useEffect, useRef, useState } from "react";
import * as Blockly from 'blockly';
import { useBlockly } from "./hooks/useBlockly";
import { BlocklyWorkspace, ExecutionMode } from "./types";
import { registerPLCBlocks } from "./blocklyBlocks";
import { PropertiesPanel } from "./PropertiesPanel";
import { CodePanel } from "./CodePanel";
import "./styles.css";
import "./blocklyTheme.css";

/**
 * Main Blockly Editor Component
 * Provides visual programming interface for PLC programs
 */
export const BlocklyEditor: React.FC = () => {
  console.log('[BlocklyEditor webview] Component rendering');
  
  const {
    workspace,
    generatedCode,
    executionMode,
    isExecuting,
    errors,
    saveWorkspace,
    generateCode,
    startExecution,
    stopExecution,
  } = useBlockly();

  const workspaceRef = useRef<HTMLDivElement>(null);
  const blocklyWorkspaceRef = useRef<Blockly.WorkspaceSvg | null>(null);
  const [selectedBlockId, setSelectedBlockId] = useState<string | null>(null);
  const [showCode, setShowCode] = useState(false);
  const [showProperties, setShowProperties] = useState(true);
  const [selectedMode, setSelectedMode] = useState<ExecutionMode>("simulation");

  // Initialize Blockly workspace
  useEffect(() => {
    if (!workspaceRef.current || blocklyWorkspaceRef.current) return;

    // Register custom PLC blocks
    registerPLCBlocks();

    // Create Blockly workspace
    const blocklyWorkspace = Blockly.inject(workspaceRef.current, {
      toolbox: getToolboxXML(),
      grid: {
        spacing: 20,
        length: 3,
        colour: '#ccc',
        snap: true
      },
      zoom: {
        controls: true,
        wheel: true,
        startScale: 1.0,
        maxScale: 3,
        minScale: 0.3,
        scaleSpeed: 1.2
      },
      trashcan: true,
      move: {
        scrollbars: {
          horizontal: true,
          vertical: true
        },
        drag: true,
        wheel: true
      }
    });

    blocklyWorkspaceRef.current = blocklyWorkspace;
    (window as any).blocklyWorkspace = blocklyWorkspace;
    console.log('[BlocklyEditor] Blockly workspace stored in window');

    // Listen for workspace changes (but ignore during programmatic loads)
    blocklyWorkspace.addChangeListener((event: Blockly.Events.Abstract) => {
      // Only save on user-initiated changes
      if (event.type === Blockly.Events.BLOCK_CREATE ||
          event.type === Blockly.Events.BLOCK_DELETE ||
          event.type === Blockly.Events.BLOCK_CHANGE ||
          event.type === Blockly.Events.BLOCK_MOVE) {
        
        // Don't save during programmatic loads
        if (Blockly.Events.getGroup()) {
          return;
        }
        
        // Serialize workspace and save
        const json = Blockly.serialization.workspaces.save(blocklyWorkspace);
        
        // Preserve metadata from current workspace
        saveWorkspace({
          blocks: json.blocks || {},
          variables: json.variables || [],
          metadata: workspace?.metadata || { name: 'Untitled', description: '' }
        });
      }
    });

    console.log("Blockly workspace initialized");

    return () => {
      blocklyWorkspace.dispose();
      blocklyWorkspaceRef.current = null;
      (window as any).blocklyWorkspace = null;
      console.log("Blockly workspace cleanup");
    };
  }, []);

  // Update workspace when data changes
  useEffect(() => {
    if (!workspace || !blocklyWorkspaceRef.current) return;
    
    try {
      // Clear existing workspace first
      blocklyWorkspaceRef.current.clear();
      
      // Prepare the state object for Blockly serialization
      // Blockly expects: { blocks: {...}, variables: [...] }
      // Our JSON has: { blocks: {...}, variables: [...], metadata: {...} }
      const blocklyState = {
        blocks: workspace.blocks,
        variables: workspace.variables || []
      };
      
      console.log("Loading workspace from JSON:", blocklyState);
      
      // Disable events during load to prevent triggering save
      Blockly.Events.disable();
      Blockly.serialization.workspaces.load(blocklyState, blocklyWorkspaceRef.current);
      Blockly.Events.enable();
      
      console.log("✅ Workspace loaded successfully");
      console.log("Total blocks in workspace:", blocklyWorkspaceRef.current.getAllBlocks(false).length);
    } catch (error) {
      Blockly.Events.enable(); // Re-enable events even if error
      console.error("❌ Failed to load workspace:", error);
      console.error("Workspace data:", workspace);
    }
  }, [workspace]);

  const handleGenerateCode = () => {
    generateCode();
    setShowCode(true);
  };

  const handleStartExecution = () => {
    startExecution(selectedMode);
  };

  const handleStopExecution = () => {
    stopExecution();
  };

  const handleBlockSelected = (blockId: string) => {
    setSelectedBlockId(blockId);
  };

  // Define toolbox structure (blocks available for dragging)
  const getToolboxXML = () => {
    return {
      kind: 'categoryToolbox',
      contents: [
        {
          kind: 'category',
          name: 'Logic',
          colour: '210',
          contents: [
            { kind: 'block', type: 'controls_if' },
            { kind: 'block', type: 'logic_compare' },
            { kind: 'block', type: 'logic_operation' },
            { kind: 'block', type: 'logic_negate' },
            { kind: 'block', type: 'logic_boolean' },
          ]
        },
        {
          kind: 'category',
          name: 'Loops',
          colour: '120',
          contents: [
            { kind: 'block', type: 'controls_whileUntil' },
            { kind: 'block', type: 'controls_for' },
            { kind: 'block', type: 'controls_forEach' },
            { kind: 'block', type: 'controls_flow_statements' },
          ]
        },
        {
          kind: 'category',
          name: 'Math',
          colour: '230',
          contents: [
            { kind: 'block', type: 'math_number' },
            { kind: 'block', type: 'math_arithmetic' },
            { kind: 'block', type: 'math_single' },
            { kind: 'block', type: 'math_trig' },
            { kind: 'block', type: 'math_constant' },
            { kind: 'block', type: 'math_number_property' },
            { kind: 'block', type: 'math_change' },
            { kind: 'block', type: 'math_round' },
          ]
        },
        {
          kind: 'category',
          name: 'Variables',
          colour: '330',
          custom: 'VARIABLE'
        },
        {
          kind: 'category',
          name: 'Functions',
          colour: '290',
          custom: 'PROCEDURE'
        },
        {
          kind: 'category',
          name: 'PLC I/O',
          colour: '160',
          contents: [
            { kind: 'block', type: 'io_digital_write' },
            { kind: 'block', type: 'io_digital_read' },
          ]
        },
        {
          kind: 'category',
          name: 'PLC Timers',
          colour: '65',
          contents: [
            { kind: 'block', type: 'timer_ton' },
          ]
        },
        {
          kind: 'category',
          name: 'PLC Counters',
          colour: '20',
          contents: [
            { kind: 'block', type: 'counter_ctu' },
          ]
        },
        {
          kind: 'category',
          name: 'Comments',
          colour: '160',
          contents: [
            { kind: 'block', type: 'comment' },
          ]
        },
      ]
    };
  };

  return (
    <div className="blockly-editor-container">
      {/* Toolbar */}
      <div className="blockly-toolbar">
        <div className="toolbar-section">
          <h2 className="editor-title">Blockly PLC Editor</h2>
          {workspace?.metadata?.name && (
            <span className="workspace-name">({workspace.metadata.name})</span>
          )}
        </div>

        <div className="toolbar-section">
          <button
            className="toolbar-button"
            onClick={handleGenerateCode}
            disabled={!workspace}
            title="Generate ST Code"
          >
            <span>⚙️</span> Generate Code
          </button>

          {/* Execution Mode Selector (only when not executing) */}
          {!isExecuting && (
            <>
              <button
                className={`toolbar-button ${selectedMode === "simulation" ? "active" : ""}`}
                onClick={() => setSelectedMode("simulation")}
                title="Simulation mode (logged to console)"
              >
                <span>🖥️</span> Simulation
              </button>
              <button
                className={`toolbar-button ${selectedMode === "hardware" ? "active" : ""}`}
                onClick={() => setSelectedMode("hardware")}
                title="Hardware mode (real I/O via trust-runtime)"
              >
                <span>🔌</span> Hardware
              </button>
            </>
          )}

          {/* Run/Stop Button */}
          {!isExecuting ? (
            <button
              className="toolbar-button success"
              onClick={handleStartExecution}
              disabled={!workspace}
              title={`Run in ${selectedMode} mode`}
            >
              <span>▶️</span> Run
            </button>
          ) : (
            <>
              <button
                className="toolbar-button danger"
                onClick={handleStopExecution}
                title="Stop execution"
              >
                <span>⏹️</span> Stop
              </button>
              <span className="execution-indicator">
                {executionMode === "simulation" ? "🖥️ Simulating" : "🔌 Running on Hardware"}
              </span>
            </>
          )}

          <button
            className="toolbar-button"
            onClick={() => setShowCode(!showCode)}
            title="Toggle code view"
          >
            <span>{showCode ? "📦" : "📝"}</span>
            {showCode ? "Blocks" : "Code"}
          </button>

          <button
            className="toolbar-button"
            onClick={() => setShowProperties(!showProperties)}
            title="Toggle properties panel"
          >
            <span>⚙️</span> Properties
          </button>
        </div>
      </div>

      {/* Execution Status */}
      {isExecuting && (
        <div className={`execution-banner ${executionMode}`}>
          <span className="status-indicator">●</span>
          Running in {executionMode} mode
        </div>
      )}

      {/* Main Content Area */}
      <div className="blockly-content">
        {/* Center Panel - Workspace or Generated Code */}
        <div className="blockly-workspace-container">
          {showCode ? (
            <CodePanel code={generatedCode} errors={errors} />
          ) : (
            <div
              ref={workspaceRef}
              className="blockly-workspace"
              id="blocklyDiv"
            >
              {!workspace && (
                <div className="workspace-placeholder">
                  <p>Loading Blockly workspace...</p>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Right Panel - Properties */}
        {showProperties && (
          <div className="blockly-properties-container">
            <PropertiesPanel
              workspace={workspace}
              selectedBlockId={selectedBlockId}
              onWorkspaceChange={saveWorkspace}
            />
          </div>
        )}
      </div>

      {/* Status Bar */}
      <div className="blockly-status-bar">
        <span>
          Blocks: {workspace?.blocks?.blocks?.length || 0} |
          Variables: {workspace?.variables?.length || 0}
        </span>
        {errors.length > 0 && (
          <span className="error-count">⚠️ {errors.length} warnings</span>
        )}
      </div>
    </div>
  );
};
