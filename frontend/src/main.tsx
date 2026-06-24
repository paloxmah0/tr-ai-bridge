import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import "./index.css";
import Layout from "./pages/Layout";
import AITrade from "./pages/AITrade";
import Accounts from "./pages/Accounts";
import Strategies from "./pages/Strategies";
import Notes from "./pages/Notes";
import Backtest from "./pages/Backtest";
import SignalsTrades from "./pages/SignalsTrades";
import Analytics from "./pages/Analytics";
import Settings from "./pages/Settings";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter>
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<AITrade />} />
          <Route path="accounts/:id" element={<Accounts />} />
          <Route path="strategies" element={<Strategies />} />
          <Route path="notes" element={<Notes />} />
          <Route path="backtest" element={<Backtest />} />
          <Route path="activity" element={<SignalsTrades />} />
          <Route path="analytics" element={<Analytics />} />
          <Route path="settings" element={<Settings />} />
        </Route>
      </Routes>
    </BrowserRouter>
  </StrictMode>
);
