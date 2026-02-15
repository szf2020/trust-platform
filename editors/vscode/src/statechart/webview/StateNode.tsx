import React, { memo } from "react";
import { Handle, Position, NodeProps } from "@xyflow/react";
import { StateNodeData } from "./types";

/**
 * Visual component for a state node in the StateChart  * Renders different styles based on state type (normal, initial, final, compound)
 */
export const StateNode: React.FC<NodeProps<StateNodeData>> = memo(
  ({ data, selected }) => {
    const getNodeStyle = () => {
      const baseStyle: React.CSSProperties = {
        padding: "12px 16px",
        borderRadius: "8px",
        border: `2px solid ${selected ? "var(--vscode-focusBorder)" : "var(--vscode-panel-border)"}`,
        backgroundColor: "var(--vscode-editor-background)",
        minWidth: "120px",
        fontSize: "13px",
        fontFamily: "var(--vscode-font-family)",
        color: "var(--vscode-editor-foreground)",
        boxShadow: selected
          ? "0 0 0 2px var(--vscode-focusBorder)"
          : "0 2px 4px rgba(0, 0, 0, 0.1)",
        transition: "all 0.3s ease",
      };

      // Active state styling - override everything for maximum visibility
      if (data.isActive) {
        return {
          ...baseStyle,
          backgroundColor: "rgba(76, 175, 80, 0.25)", // Green background with transparency
          borderColor: "#4caf50", // Bright green border
          borderWidth: "3px",
          boxShadow: "0 0 12px rgba(76, 175, 80, 0.6), 0 0 4px rgba(76, 175, 80, 0.8)",
          color: "#ffffff", // White text for contrast
          fontWeight: 600,
          transform: "scale(1.05)", // Slightly larger
        };
      }

      // Style variations by state type
      switch (data.type) {
        case "initial":
          return {
            ...baseStyle,
            borderColor: "var(--vscode-testing-iconPassed, #4caf50)",
            borderWidth: "3px",
          };
        case "final":
          return {
            ...baseStyle,
            borderColor: "var(--vscode-testing-iconFailed, #f44336)",
            borderWidth: "3px",
            background: "var(--vscode-editor-inactiveSelectionBackground)",
          };
        case "compound":
          return {
            ...baseStyle,
            borderColor: "var(--vscode-button-background, #0e639c)",
            borderStyle: "dashed",
          };
        default:
          return baseStyle;
      }
    };

    const hasEntry = data.entry && data.entry.length > 0;
    const hasExit = data.exit && data.exit.length > 0;

    return (
      <div style={getNodeStyle()}>
        {/* Input handle for incoming transitions */}
        <Handle
          type="target"
          position={Position.Top}
          style={{
            background: "var(--vscode-button-background)",
            width: "10px",
            height: "10px",
            border: "2px solid var(--vscode-editor-background)",
          }}
        />

        {/* State label and type indicator */}
        <div style={{ marginBottom: hasEntry || hasExit ? "8px" : 0 }}>
          <div
            style={{
              fontWeight: data.isActive ? 700 : 600,
              fontSize: data.isActive ? "15px" : "14px",
              marginBottom: "4px",
              color: data.isActive ? "#ffffff" : "inherit",
            }}
          >
            {data.isActive && "▶ "}{data.label}
          </div>
          {data.type !== "normal" && (
            <div
              style={{
                fontSize: "11px",
                opacity: data.isActive ? 0.9 : 0.7,
                textTransform: "uppercase",
                color: data.isActive ? "#e0f2e0" : "inherit",
              }}
            >
              {data.type}
            </div>
          )}
        </div>

        {/* Entry actions */}
        {hasEntry && (
          <div
            style={{
              fontSize: "11px",
              marginTop: "6px",
              paddingTop: "6px",
              borderTop: `1px solid ${data.isActive ? "rgba(255,255,255,0.3)" : "var(--vscode-panel-border)"}`,
              color: data.isActive ? "#e0f2e0" : "inherit",
            }}
          >
            <div style={{ opacity: data.isActive ? 0.9 : 0.7, marginBottom: "2px" }}>entry /</div>
            {data.entry!.map((action, idx) => (
              <div
                key={idx}
                style={{
                  paddingLeft: "8px",
                  fontFamily: "var(--vscode-editor-font-family)",
                  color: data.isActive ? "#ffffff" : "inherit",
                }}
              >
                {action}
              </div>
            ))}
          </div>
        )}

        {/* Exit actions */}
        {hasExit && (
          <div
            style={{
              fontSize: "11px",
              marginTop: "6px",
              paddingTop: "6px",
              borderTop: `1px solid ${data.isActive ? "rgba(255,255,255,0.3)" : "var(--vscode-panel-border)"}`,
              color: data.isActive ? "#e0f2e0" : "inherit",
            }}
          >
            <div style={{ opacity: data.isActive ? 0.9 : 0.7, marginBottom: "2px" }}>exit /</div>
            {data.exit!.map((action, idx) => (
              <div
                key={idx}
                style={{
                  paddingLeft: "8px",
                  fontFamily: "var(--vscode-editor-font-family)",
                  color: data.isActive ? "#ffffff" : "inherit",
                }}
              >
                {action}
              </div>
            ))}
          </div>
        )}

        {/* Active state indicator - enhanced pulse animation */}
        {data.isActive && (
          <>
            <div
              style={{
                position: "absolute",
                top: "-8px",
                right: "-8px",
                width: "20px",
                height: "20px",
                borderRadius: "50%",
                backgroundColor: "#4caf50",
                border: "3px solid #ffffff",
                animation: "pulse 1.5s infinite, glow 2s infinite",
                zIndex: 10,
              }}
            />
            {/* Outer glow ring */}
            <div
              style={{
                position: "absolute",
                top: "-14px",
                right: "-14px",
                width: "32px",
                height: "32px",
                borderRadius: "50%",
                border: "2px solid #4caf50",
                animation: "ripple 2s infinite",
                zIndex: 9,
              }}
            />
          </>
        )}

        {/* Output handle for outgoing transitions */}
        <Handle
          type="source"
          position={Position.Bottom}
          style={{
            background: "var(--vscode-button-background)",
            width: "10px",
            height: "10px",
            border: "2px solid var(--vscode-editor-background)",
          }}
        />

        <style>{`
          @keyframes pulse {
            0%, 100% { 
              transform: scale(1);
              opacity: 1;
            }
            50% { 
              transform: scale(1.2);
              opacity: 0.8;
            }
          }
          
          @keyframes glow {
            0%, 100% {
              box-shadow: 0 0 5px #4caf50, 0 0 10px #4caf50;
            }
            50% {
              box-shadow: 0 0 10px #4caf50, 0 0 20px #4caf50, 0 0 30px rgba(76, 175, 80, 0.5);
            }
          }
          
          @keyframes ripple {
            0% {
              transform: scale(1);
              opacity: 1;
            }
            100% {
              transform: scale(1.8);
              opacity: 0;
            }
          }
        `}</style>
      </div>
    );
  }
);

StateNode.displayName = "StateNode";
