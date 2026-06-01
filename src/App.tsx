import { HashRouter, Route, Routes } from "react-router-dom";
import { AppLayout } from "@/components/layout/AppLayout";
import { AccountsPage } from "@/features/accounts/AccountsPage";
import { BackupPage } from "@/features/backup/BackupPage";
import { JobsPage } from "@/features/jobs/JobsPage";
import { BrowsePage } from "@/features/browse/BrowsePage";
import { ExtractPage } from "@/features/extract/ExtractPage";
import { SettingsPage } from "@/features/settings/SettingsPage";

export default function App() {
  return (
    <HashRouter>
      <Routes>
        <Route element={<AppLayout />}>
          <Route index element={<AccountsPage />} />
          <Route path="backup" element={<BackupPage />} />
          <Route path="jobs" element={<JobsPage />} />
          <Route path="browse" element={<BrowsePage />} />
          <Route path="extract" element={<ExtractPage />} />
          <Route path="settings" element={<SettingsPage />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
