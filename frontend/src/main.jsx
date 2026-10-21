import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App.jsx";
import { apiUrl } from "./utils/api.js";
import "./index.css";

async function startApplication() {
  try {
    const response = await fetch(apiUrl("/me"), { credentials: "include" });
    if (response.status === 401) {
      window.location.replace(apiUrl("/auth/login"));
      return;
    }
    if (!response.ok) throw new Error(`Session check failed: ${response.status}`);

    createRoot(document.getElementById("root")).render(
      <StrictMode>
        <BrowserRouter basename={import.meta.env.BASE_URL.replace(/\/$/, "")}>
          <App />
        </BrowserRouter>
      </StrictMode>,
    );
  } catch (error) {
    console.error("Unable to verify the current session", error);
    document.getElementById("root").textContent = "Unable to connect to the application.";
  }
}

startApplication();
