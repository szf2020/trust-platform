import React, { useState, useRef, useEffect } from "react";
import { Stage, Layer } from "react-konva";
import { Contact } from "./elements/Contact";
import { Coil } from "./elements/Coil";
import { Rung } from "./elements/Rung";
import { Toolbar } from "./Toolbar";
import type { LadderProgram, Rung as RungType, Contact as ContactType, Coil as CoilType } from "../ladderEngine";

declare const vscode: any;

export function LadderEditor() {
  const [program, setProgram] = useState<LadderProgram>({
    rungs: [],
    variables: [],
    metadata: {
      name: "New Ladder Program",
      description: "Ladder logic program"
    }
  });
  
  const [selectedTool, setSelectedTool] = useState<string | null>(null);
  const [selectedMode, setSelectedMode] = useState<"simulation" | "hardware">("simulation");
  const [isExecuting, setIsExecuting] = useState(false);
  const stageRef = useRef<any>(null);

  const STAGE_WIDTH = 1200;
  const STAGE_HEIGHT = 800;
  const RUNG_HEIGHT = 100;
  const LEFT_RAIL_X = 50;
  const RIGHT_RAIL_X = 1100;

  // Initialize with one empty rung
  useEffect(() => {
    if (program.rungs.length === 0) {
      addRung();
    }
  }, []);

  // Handle messages from extension
  useEffect(() => {
    const messageHandler = (event: MessageEvent) => {
      const message = event.data;
      
      switch (message.type) {
        case "loadProgram":
          setProgram(message.program);
          break;
        case "executionStarted":
          setIsExecuting(true);
          break;
        case "executionStopped":
          setIsExecuting(false);
          break;
      }
    };

    window.addEventListener("message", messageHandler);
    return () => window.removeEventListener("message", messageHandler);
  }, []);

  const addRung = () => {
    const newRung: RungType = {
      id: `rung_${Date.now()}`,
      y: program.rungs.length * RUNG_HEIGHT + 100,
      elements: [],
      connections: []
    };
    
    setProgram(prev => ({
      ...prev,
      rungs: [...prev.rungs, newRung]
    }));
  };

  const handleStageClick = (e: any) => {
    const stage = e.target.getStage();
    const pointerPosition = stage.getPointerPosition();
    
    if (!selectedTool || !pointerPosition) return;

    // Find which rung was clicked
    const clickedRungIndex = Math.floor((pointerPosition.y - 50) / RUNG_HEIGHT);
    if (clickedRungIndex < 0 || clickedRungIndex >= program.rungs.length) return;

    const rung = program.rungs[clickedRungIndex];
    const elementId = `${selectedTool}_${Date.now()}`;

    // Add element to the rung
    if (selectedTool === "contact") {
      const newContact: ContactType = {
        id: elementId,
        type: "contact",
        contactType: "NO",
        variable: "%IX0.0",
        position: { x: pointerPosition.x, y: rung.y }
      };
      
      const updatedRungs = [...program.rungs];
      updatedRungs[clickedRungIndex].elements.push(newContact);
      setProgram(prev => ({ ...prev, rungs: updatedRungs }));
      
    } else if (selectedTool === "coil") {
      const newCoil: CoilType = {
        id: elementId,
        type: "coil",
        coilType: "NORMAL",
        variable: "%QX0.0",
        position: { x: pointerPosition.x, y: rung.y }
      };
      
      const updatedRungs = [...program.rungs];
      updatedRungs[clickedRungIndex].elements.push(newCoil);
      setProgram(prev => ({ ...prev, rungs: updatedRungs }));
    }

    // Clear tool selection after placing element
    setSelectedTool(null);
  };

  const handleElementDragEnd = (rungIndex: number, elementId: string, newPos: { x: number; y: number }) => {
    const updatedRungs = [...program.rungs];
    const element = updatedRungs[rungIndex].elements.find(el => el.id === elementId);
    if (element) {
      element.position = newPos;
      setProgram(prev => ({ ...prev, rungs: updatedRungs }));
    }
  };

  const handleRun = () => {
    if (typeof vscode !== 'undefined') {
      vscode.postMessage({
        type: selectedMode === "simulation" ? "runSimulation" : "runHardware",
        program
      });
    }
  };

  const handleStop = () => {
    if (typeof vscode !== 'undefined') {
      vscode.postMessage({
        type: "stop"
      });
    }
    setIsExecuting(false);
  };

  const handleSave = () => {
    if (typeof vscode !== 'undefined') {
      vscode.postMessage({
        type: "save",
        program
      });
    }
  };

  return (
    <div className="ladder-editor">
      <Toolbar
        selectedTool={selectedTool}
        onToolSelect={setSelectedTool}
        selectedMode={selectedMode}
        onModeSelect={setSelectedMode}
        isExecuting={isExecuting}
        onRun={handleRun}
        onStop={handleStop}
        onAddRung={addRung}
        onSave={handleSave}
      />
      
      <div className="canvas-container">
        <Stage
          ref={stageRef}
          width={STAGE_WIDTH}
          height={STAGE_HEIGHT}
          onClick={handleStageClick}
        >
          <Layer>
            {/* Draw rungs */}
            {program.rungs.map((rung, index) => (
              <Rung
                key={rung.id}
                y={rung.y}
                leftX={LEFT_RAIL_X}
                rightX={RIGHT_RAIL_X}
                rungNumber={index + 1}
              />
            ))}
            
            {/* Draw elements on rungs */}
            {program.rungs.map((rung, rungIndex) => (
              <React.Fragment key={rung.id}>
                {rung.elements.map(element => {
                  if (element.type === 'contact') {
                    return (
                      <Contact
                        key={element.id}
                        x={element.position.x}
                        y={element.position.y}
                        contactType={element.contactType}
                        variable={element.variable}
                        onDragEnd={(e) => {
                          const newPos = { x: e.target.x(), y: e.target.y() };
                          handleElementDragEnd(rungIndex, element.id, newPos);
                        }}
                      />
                    );
                  } else if (element.type === 'coil') {
                    return (
                      <Coil
                        key={element.id}
                        x={element.position.x}
                        y={element.position.y}
                        coilType={element.coilType}
                        variable={element.variable}
                        onDragEnd={(e) => {
                          const newPos = { x: e.target.x(), y: e.target.y() };
                          handleElementDragEnd(rungIndex, element.id, newPos);
                        }}
                      />
                    );
                  }
                  return null;
                })}
              </React.Fragment>
            ))}
          </Layer>
        </Stage>
      </div>

      <div className="status-bar">
        {isExecuting && <span className="execution-indicator">● Executing</span>}
        <span>Mode: {selectedMode}</span>
        <span>Rungs: {program.rungs.length}</span>
        {selectedTool && <span>Selected tool: {selectedTool}</span>}
      </div>
    </div>
  );
}
