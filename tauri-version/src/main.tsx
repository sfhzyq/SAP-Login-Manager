import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/global.css";

// 首帧即应用上次缓存的主题，避免登录/解锁页出现亮色闪烁
try {
  const cachedTheme = localStorage.getItem("app-theme");
  if (cachedTheme) document.documentElement.setAttribute("data-theme", cachedTheme);
} catch {}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
