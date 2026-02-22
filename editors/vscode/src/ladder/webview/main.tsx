import React from "react";
import ReactDOM from "react-dom/client";
import { LadderEditor } from "./LadderEditor";
import "./styles.css";

const root = ReactDOM.createRoot(document.getElementById("root")!);
root.render(
  <React.StrictMode>
    <LadderEditor />
  </React.StrictMode>
);
