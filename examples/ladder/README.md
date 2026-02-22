# Ladder Logic Editor - Prototype

🚧 **Work in Progress** - Initial prototype

## Overview

Visual Ladder Logic editor for IEC 61131-3 ladder programming within VS Code.

## Current Features (Prototype v0.1)

- ✅ Canvas-based editor using **Konva.js** + React
- ✅ Draggable ladder elements:
  - Contact (NO/NC typically Open/Closed)
  - Coil (Normal/Set/Reset/Negated)
- ✅ Rung management (add/remove)
- ✅ Power rails rendering
- ✅ Simulation/Hardware mode selector
- ✅ Run/Stop controls

## Architecture

```
editors/vscode/src/ladder/
├── ladderEngine.ts          # Type definitions
├── webview/
│   ├── LadderEditor.tsx     # Main canvas component
│   ├── Toolbar.tsx          # Tools & controls
│   ├── elements/
│   │   ├── Contact.tsx      # Contact element
│   │   ├── Coil.tsx         # Coil element
│   │   └── Rung.tsx         # Rung line
│   ├── main.tsx             # Entry point
│   └── styles.css           # Styling
```

## Tech Stack

- **Konva.js** 9.3.6 - Canvas rendering
- **react-konva** 18.2.10 - React bindings
- **React** 19.2.4 - UI framework
- **TypeScript** 5.0 - Type safety
- **esbuild** - Fast bundling

## Building

```bash
cd editors/vscode
npm install
npm run build:ladder
```

## Example Programs

- `examples/ladder/simple-start-stop.ladder.json` - Basic start/stop logic

## Roadmap

### Phase 1: Core Editor (Current)
- [x] Canvas rendering with Konva
- [x] Contact & Coil elements
- [x] Drag & drop
- [ ] Element connections (wiring)
- [ ] Element properties panel
- [ ] Delete/edit elements

### Phase 2: More Elements
- [ ] Timer blocks (TON/TOF/TP)
- [ ] Counter blocks (CTU/CTD/CTUD)
- [ ] Parallel branches
- [ ] Series connections

### Phase 3: Execution
- [ ] Ladder interpreter or ST generator
- [ ] RuntimeClient integration (reuse from Blockly)
- [ ] Hardware execution via `hardware_8do` backend
- [ ] Element highlighting during execution

### Phase 4: Professional Features
- [ ] Auto-routing connections
- [ ] Snap to grid
- [ ] Undo/redo
- [ ] Copy/paste rungs
- [ ] Search/replace variables
- [ ] Export to PDF/image

## Design Decisions

**Why Konva.js over Fabric.js?**
- Better React integration (react-konva)
- Native TypeScript support
- Better performance
- Cleaner API

**Why interpreted execution?**
- Same pattern as Blockly editor
- Real-time execution visibility
- Simpler debugging

## Contributing

This is a prototype. Feedback welcome!

## License

MIT OR Apache-2.0 (same as main project)
