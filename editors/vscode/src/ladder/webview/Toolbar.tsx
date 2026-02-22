import React from "react";

interface ToolbarProps {
  selectedTool: string | null;
  onToolSelect: (tool: string | null) => void;
  selectedMode: "simulation" | "hardware";
  onModeSelect: (mode: "simulation" | "hardware") => void;
  isExecuting: boolean;
  onRun: () => void;
  onStop: () => void;
  onAddRung: () => void;
  onSave: () => void;
}

export function Toolbar({
  selectedTool,
  onToolSelect,
  selectedMode,
  onModeSelect,
  isExecuting,
  onRun,
  onStop,
  onAddRung,
  onSave
}: ToolbarProps) {
  return (
    <div className="toolbar">
      <div className="toolbar-section">
        <h3>Elements</h3>
        <button
          className={`toolbar-button ${selectedTool === 'contact' ? 'active' : ''}`}
          onClick={() => onToolSelect(selectedTool === 'contact' ? null : 'contact')}
          title="Add Contact (NO/NC)"
        >
          ├─┤ Contact
        </button>
        <button
          className={`toolbar-button ${selectedTool === 'coil' ? 'active' : ''}`}
          onClick={() => onToolSelect(selectedTool === 'coil' ? null : 'coil')}
          title="Add Coil"
        >
          ( ) Coil
        </button>
        <button
          className={`toolbar-button ${selectedTool === 'timer' ? 'active' : ''}`}
          onClick={() => onToolSelect(selectedTool === 'timer' ? null : 'timer')}
          title="Add Timer"
          disabled
        >
          [T] Timer
        </button>
        <button
          className={`toolbar-button ${selectedTool === 'counter' ? 'active' : ''}`}
          onClick={() => onToolSelect(selectedTool === 'counter' ? null : 'counter')}
          title="Add Counter"
          disabled
        >
          [C] Counter
        </button>
      </div>

      <div className="toolbar-section">
        <h3>Rungs</h3>
        <button
          className="toolbar-button"
          onClick={onAddRung}
          title="Add new rung"
        >
          ➕ Add Rung
        </button>
      </div>

      <div className="toolbar-section">
        <h3>Mode</h3>
        <button
          className={`toolbar-button ${selectedMode === 'simulation' ? 'active' : ''}`}
          onClick={() => onModeSelect('simulation')}
        >
          🖥️ Simulation
        </button>
        <button
          className={`toolbar-button ${selectedMode === 'hardware' ? 'active' : ''}`}
          onClick={() => onModeSelect('hardware')}
        >
          ⚡ Hardware
        </button>
      </div>

      <div className="toolbar-section">
        <h3>Control</h3>
        {!isExecuting ? (
          <button
            className="toolbar-button success"
            onClick={onRun}
            title="Run program"
          >
            ▶️ Run
          </button>
        ) : (
          <button
            className="toolbar-button danger"
            onClick={onStop}
            title="Stop program"
          >
            ⏹️ Stop
          </button>
        )}
        <button
          className="toolbar-button"
          onClick={onSave}
          title="Save program"
        >
          💾 Save
        </button>
      </div>
    </div>
  );
}
