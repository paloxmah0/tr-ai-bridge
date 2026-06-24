import { NavLink, Outlet } from "react-router-dom";
import { Brain, LayoutDashboard, FileCode, StickyNote, FlaskConical, Activity, BarChart3, Settings as SettingsIcon } from "lucide-react";

const nav = [
  { to: "/", label: "AI Trade", icon: Brain, end: true },
  { to: "/accounts/:id", label: "Accounts", icon: LayoutDashboard },
  { to: "/strategies", label: "Strategies", icon: FileCode },
  { to: "/notes", label: "Notes", icon: StickyNote },
  { to: "/backtest", label: "Backtest", icon: FlaskConical },
  { to: "/activity", label: "Activity", icon: Activity },
  { to: "/analytics", label: "Analytics", icon: BarChart3 },
  { to: "/settings", label: "Settings", icon: SettingsIcon },
];

export default function Layout() {
  return (
    <div className="flex min-h-screen">
      <aside className="w-56 shrink-0 bg-ink-900 border-r border-ink-700 flex flex-col">
        <div className="px-5 py-4 border-b border-ink-700">
          <h1 className="text-lg font-bold text-white flex items-center gap-2"><Brain size={20} className="text-accent" /> Trading</h1>
          <p className="text-xs text-muted">AI Strategy Engine</p>
        </div>
        <nav className="flex-1 py-2">
          {nav.map((n) => (
            <NavLink
              key={n.to}
              to={n.to}
              end={n.end}
              className={({ isActive }) =>
                `flex items-center gap-3 px-5 py-2.5 text-sm transition-colors ${
                  isActive ? "bg-ink-800 text-accent border-r-2 border-accent" : "text-gray-400 hover:text-gray-200 hover:bg-ink-850"
                }`
              }
            >
              <n.icon size={17} />
              {n.label}
            </NavLink>
          ))}
        </nav>
      </aside>
      <main className="flex-1 overflow-auto p-6">
        <Outlet />
      </main>
    </div>
  );
}
