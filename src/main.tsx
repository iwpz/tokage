import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import "./i18n";
import App from "./App";
import { hydrate } from "./lib/db";

// Stores are restored from the database before the first render.
void hydrate().finally(() => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  // Explicit acceptance bundles only; normal bundles contain no driver.
  if (import.meta.env.VITE_TOKAGE_LIVE_ACCEPTANCE === "1") {
    void import("./lib/liveDesktopAcceptance").then(({ runLiveDesktopAcceptance }) => runLiveDesktopAcceptance());
  }
});
