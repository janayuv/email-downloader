import { NavLink } from "react-router-dom";
import { Mail, Users, DownloadCloud, ListChecks, Search, FileDown, Settings } from "lucide-react";
import { cn } from "@/lib/utils";

const links = [
  { to: "/", label: "Accounts", icon: Users, end: true },
  { to: "/backup", label: "Backup", icon: DownloadCloud, end: false },
  { to: "/jobs", label: "Jobs", icon: ListChecks, end: false },
  { to: "/browse", label: "Browse", icon: Search, end: false },
  { to: "/extract", label: "Extract", icon: FileDown, end: false },
  { to: "/settings", label: "Settings", icon: Settings, end: false },
];

export function Sidebar() {
  return (
    <aside className="flex h-full w-60 flex-col border-r bg-card">
      <div className="flex items-center gap-2 px-5 py-4">
        <Mail className="h-6 w-6 text-primary" />
        <div>
          <div className="text-sm font-semibold leading-tight">Email Downloader</div>
          <div className="text-xs text-muted-foreground">Workspace Backup</div>
        </div>
      </div>
      <nav className="flex-1 space-y-1 px-3 py-2">
        {links.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                isActive
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
              )
            }
          >
            <Icon className="h-4 w-4" />
            {label}
          </NavLink>
        ))}
      </nav>
      <div className="px-5 py-3 text-xs text-muted-foreground">v0.1.0 · Milestone 1</div>
    </aside>
  );
}
