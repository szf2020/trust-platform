import React, { useState } from "react";
import { ExecutionState, ExecutionMode } from "./types";

interface ExecutionPanelProps {
  executionState: ExecutionState | null;
  isRunning: boolean;
  onStart: (mode: ExecutionMode) => void;
  onStop: () => void;
  onSendEvent: (event: string) => void;
}

export const ExecutionPanel: React.FC<ExecutionPanelProps> = ({
  executionState,
  isRunning,
  onStart,
  onStop,
  onSendEvent,
}) => {
  const [customEvent, setCustomEvent] = useState("");
  const [executionMode, setExecutionMode] = useState<ExecutionMode>("simulation");
  const [isCollapsed, setIsCollapsed] = useState(false);

  const handleSendCustomEvent = () => {
    if (customEvent.trim()) {
      onSendEvent(customEvent.trim());
      setCustomEvent("");
    }
  };

  const handleStartExecution = () => {
    onStart(executionMode);
  };

  const currentState = executionState?.currentState;
  const availableEvents = executionState?.availableEvents || [];
  const previousState = executionState?.previousState;
  const activeMode = executionState?.mode || executionMode;

  return (
    <div style={{
      ...panelContainerStyle,
      height: isCollapsed ? 'auto' : '300px',
      minHeight: '40px',
      borderBottom: '1px solid var(--vscode-panel-border)',
      overflow: isCollapsed ? 'visible' : 'auto',
    }}>
      {/* Collapsible Header */}
      <div
        style={{
          ...headerStyle,
          borderBottom: isCollapsed ? 'none' : '1px solid var(--vscode-panel-border)',
          cursor: 'pointer',
          userSelect: 'none',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}
        onClick={() => setIsCollapsed(!isCollapsed)}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span
            style={{
              transform: isCollapsed ? 'rotate(-90deg)' : 'rotate(0deg)',
              transition: 'transform 0.2s',
            }}
          >
            ▼
          </span>
          <span style={titleStyle}>Execution</span>
          {isRunning && (
            <span
              style={{
                backgroundColor: 'var(--vscode-testing-runAction)',
                width: '8px',
                height: '8px',
                borderRadius: '50%',
                display: 'inline-block',
              }}
            />
          )}
        </div>
      </div>

      {/* Panel Content */}
      {!isCollapsed && (
        <div style={{ padding: '12px' }}>

      {/* Execution Mode Selector */}
      {!isRunning && (
        <div style={sectionStyle}>
          <label style={labelStyle}>Execution Mode</label>
          <div style={modeToggleContainerStyle}>
            <button
              style={{
                ...modeButtonStyle,
                ...(executionMode === "simulation" ? modeButtonActiveStyle : {}),
              }}
              onClick={() => setExecutionMode("simulation")}
            >
              🖥️ Simulation
            </button>
            <button
              style={{
                ...modeButtonStyle,
                ...(executionMode === "hardware" ? modeButtonActiveStyle : {}),
              }}
              onClick={() => setExecutionMode("hardware")}
            >
              🔌 Hardware
            </button>
          </div>
          <div style={modeDescriptionStyle}>
            {executionMode === "simulation" ? (
              <span>
                💡 <strong>Mock mode:</strong> Actions logged to console only
              </span>
            ) : (
              <span>
                ⚡ <strong>Real I/O:</strong> Actions sent to trust-runtime via control endpoint
              </span>
            )}
          </div>
        </div>
      )}

      {/* Control Buttons */}
      <div style={sectionStyle}>
        <div style={buttonGroupStyle}>
          {!isRunning ? (
            <button style={runButtonStyle} onClick={handleStartExecution}>
              ▶️ Start {executionMode === "simulation" ? "Simulation" : "Hardware"}
            </button>
          ) : (
            <>
              <button style={stopButtonStyle} onClick={onStop}>
                ⏹️ Stop
              </button>
              <div style={runningModeIndicatorStyle}>
                {activeMode === "simulation" ? "🖥️ Simulating" : "🔌 Running on Hardware"}
              </div>
            </>
          )}
        </div>
      </div>

      {/* Current State Display */}
      {isRunning && executionState && (
        <>
          <div style={sectionStyle}>
            <label style={labelStyle}>Current State</label>
            <div style={currentStateBoxStyle}>
              <div style={stateIndicatorStyle}>
                <div style={activeIndicatorStyle} />
                <span style={stateNameStyle}>{currentState || "—"}</span>
              </div>
              {previousState && (
                <div style={previousStateStyle}>
                  Previous: {previousState}
                </div>
              )}
            </div>
          </div>

          {/* Available Events */}
          <div style={sectionStyle}>
            <label style={labelStyle}>Available Events</label>
            {availableEvents.length > 0 ? (
              <div style={eventsGridStyle}>
                {availableEvents.map((event) => (
                  <button
                    key={event}
                    style={eventButtonStyle}
                    onClick={() => onSendEvent(event)}
                    title={`Send ${event} event`}
                  >
                    {event}
                  </button>
                ))}
              </div>
            ) : (
              <div style={noEventsStyle}>
                No events available in current state
              </div>
            )}
          </div>

          {/* Custom Event Input */}
          <div style={sectionStyle}>
            <label style={labelStyle}>Send Custom Event</label>
            <div style={customEventInputGroupStyle}>
              <input
                style={inputStyle}
                type="text"
                value={customEvent}
                onChange={(e) => setCustomEvent(e.target.value)}
                onKeyPress={(e) => {
                  if (e.key === "Enter") {
                    handleSendCustomEvent();
                  }
                }}
                placeholder="EVENT_NAME"
              />
              <button
                style={sendButtonStyle}
                onClick={handleSendCustomEvent}
                disabled={!customEvent.trim()}
              >
                Send
              </button>
            </div>
          </div>

          {/* Execution Info */}
          {executionState.timestamp && (
            <div style={infoBoxStyle}>
              <div style={infoItemStyle}>
                <span style={infoLabelStyle}>Last Update:</span>
                <span style={infoValueStyle}>
                  {new Date(executionState.timestamp).toLocaleTimeString()}
                </span>
              </div>
            </div>
          )}
        </>
      )}

      {/* Not Running Message */}
      {!isRunning && (
        <div style={notRunningMessageStyle}>
          <div style={{ fontSize: "32px", marginBottom: "8px" }}>⏸️</div>
          <div>Press Run to start execution</div>
        </div>
      )}
        </div>
      )}
    </div>
  );
};

// Styles
const panelContainerStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "16px",
};

const headerStyle: React.CSSProperties = {
  padding: "8px 12px",
  backgroundColor: "var(--vscode-sideBarSectionHeader-background)",
};

const titleStyle: React.CSSProperties = {
  fontWeight: 600,
};

const sectionStyle: React.CSSProperties = {
  padding: "0 16px",
  display: "flex",
  flexDirection: "column",
  gap: "8px",
};

const labelStyle: React.CSSProperties = {
  fontSize: "12px",
  fontWeight: "600",
  color: "var(--vscode-editor-foreground)",
  textTransform: "uppercase",
  opacity: 0.7,
};

const buttonGroupStyle: React.CSSProperties = {
  display: "flex",
  gap: "8px",
};

const runButtonStyle: React.CSSProperties = {
  flex: 1,
  padding: "10px 16px",
  fontSize: "14px",
  fontWeight: "600",
  backgroundColor: "var(--vscode-button-background)",
  color: "var(--vscode-button-foreground)",
  border: "none",
  borderRadius: "4px",
  cursor: "pointer",
  transition: "opacity 0.2s",
};

const stopButtonStyle: React.CSSProperties = {
  ...runButtonStyle,
  backgroundColor: "var(--vscode-testing-iconFailed, #f44336)",
  color: "#ffffff",
};

const modeToggleContainerStyle: React.CSSProperties = {
  display: "flex",
  gap: "8px",
  width: "100%",
};

const modeButtonStyle: React.CSSProperties = {
  flex: 1,
  padding: "10px 12px",
  fontSize: "13px",
  fontWeight: "500",
  backgroundColor: "var(--vscode-button-secondaryBackground)",
  color: "var(--vscode-button-secondaryForeground)",
  border: "1px solid var(--vscode-button-border)",
  borderRadius: "4px",
  cursor: "pointer",
  transition: "all 0.2s",
  textAlign: "center",
};

const modeButtonActiveStyle: React.CSSProperties = {
  backgroundColor: "var(--vscode-button-background)",
  color: "var(--vscode-button-foreground)",
  borderColor: "var(--vscode-focusBorder)",
  borderWidth: "2px",
  fontWeight: "600",
};

const modeDescriptionStyle: React.CSSProperties = {
  fontSize: "11px",
  color: "var(--vscode-descriptionForeground)",
  padding: "8px 12px",
  backgroundColor: "var(--vscode-editor-inactiveSelectionBackground)",
  borderRadius: "4px",
  textAlign: "center",
};

const runningModeIndicatorStyle: React.CSSProperties = {
  flex: 1,
  padding: "10px 16px",
  fontSize: "13px",
  fontWeight: "600",
  backgroundColor: "var(--vscode-editor-inactiveSelectionBackground)",
  color: "var(--vscode-editor-foreground)",
  border: "1px solid var(--vscode-panel-border)",
  borderRadius: "4px",
  textAlign: "center",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
};

const currentStateBoxStyle: React.CSSProperties = {
  padding: "12px",
  backgroundColor: "var(--vscode-editor-inactiveSelectionBackground)",
  border: "1px solid var(--vscode-panel-border)",
  borderRadius: "4px",
};

const stateIndicatorStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "10px",
};

const activeIndicatorStyle: React.CSSProperties = {
  width: "12px",
  height: "12px",
  borderRadius: "50%",
  backgroundColor: "var(--vscode-testing-iconPassed, #4caf50)",
  animation: "pulse 2s infinite",
};

const stateNameStyle: React.CSSProperties = {
  fontSize: "16px",
  fontWeight: "600",
  color: "var(--vscode-editor-foreground)",
};

const previousStateStyle: React.CSSProperties = {
  marginTop: "8px",
  fontSize: "12px",
  opacity: 0.6,
  color: "var(--vscode-descriptionForeground)",
};

const eventsGridStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fill, minmax(120px, 1fr))",
  gap: "8px",
};

const eventButtonStyle: React.CSSProperties = {
  padding: "8px 12px",
  fontSize: "13px",
  backgroundColor: "var(--vscode-button-secondaryBackground)",
  color: "var(--vscode-button-secondaryForeground)",
  border: "1px solid var(--vscode-button-border)",
  borderRadius: "4px",
  cursor: "pointer",
  transition: "all 0.2s",
  textAlign: "center",
};

const noEventsStyle: React.CSSProperties = {
  padding: "12px",
  textAlign: "center",
  fontSize: "12px",
  color: "var(--vscode-descriptionForeground)",
  fontStyle: "italic",
};

const customEventInputGroupStyle: React.CSSProperties = {
  display: "flex",
  gap: "8px",
};

const inputStyle: React.CSSProperties = {
  flex: 1,
  padding: "6px 10px",
  fontSize: "13px",
  backgroundColor: "var(--vscode-input-background)",
  color: "var(--vscode-input-foreground)",
  border: "1px solid var(--vscode-input-border)",
  borderRadius: "2px",
  outline: "none",
};

const sendButtonStyle: React.CSSProperties = {
  padding: "6px 16px",
  fontSize: "13px",
  backgroundColor: "var(--vscode-button-background)",
  color: "var(--vscode-button-foreground)",
  border: "1px solid var(--vscode-button-border)",
  borderRadius: "2px",
  cursor: "pointer",
};

const infoBoxStyle: React.CSSProperties = {
  padding: "12px 16px",
  backgroundColor: "var(--vscode-editor-inactiveSelectionBackground)",
  borderTop: "1px solid var(--vscode-panel-border)",
  marginTop: "auto",
};

const infoItemStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  fontSize: "12px",
};

const infoLabelStyle: React.CSSProperties = {
  opacity: 0.7,
};

const infoValueStyle: React.CSSProperties = {
  fontWeight: "500",
};

const notRunningMessageStyle: React.CSSProperties = {
  flex: 1,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  color: "var(--vscode-descriptionForeground)",
  fontSize: "14px",
  padding: "32px",
  textAlign: "center",
};
