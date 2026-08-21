import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ConsoleProvider } from "./ConsoleProvider";
import { initializeThemePreference } from "./usePersistentTheme";
import "./styles.css";

initializeThemePreference();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ConsoleProvider>
      <App />
    </ConsoleProvider>
  </StrictMode>,
);
