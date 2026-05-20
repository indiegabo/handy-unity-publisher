import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import OverlayProvider from "./components/OverlayManager";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <OverlayProvider>
      <App />
    </OverlayProvider>
  </React.StrictMode>,
);
