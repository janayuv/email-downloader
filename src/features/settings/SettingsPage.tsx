import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Save, Trash, FolderOpen, RefreshCw, CheckCircle2, Download, AlertTriangle } from "lucide-react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { api } from "@/lib/ipc";
import type { AppSettings, RetentionStats } from "@/lib/types";
import { useAppStore } from "@/stores/appStore";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { formatBytes } from "@/lib/utils";

const APP_VERSION = "0.1.0";

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up-to-date" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; progress: number }
  | { kind: "ready" }
  | { kind: "error"; message: string };

export function SettingsPage() {
  const qc = useQueryClient();
  const setTheme = useAppStore((s) => s.setTheme);
  const { data } = useQuery({ queryKey: ["settings"], queryFn: api.getSettings });
  const [form, setForm] = useState<AppSettings | null>(null);
  const [stats, setStats] = useState<RetentionStats | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: "idle" });

  useEffect(() => {
    if (data) setForm(data);
  }, [data]);

  const save = useMutation({
    mutationFn: (s: AppSettings) => api.saveSettings(s),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });
  const pick = useMutation({
    mutationFn: api.pickFolder,
    onSuccess: (p) => p && form && setForm({ ...form, default_destination: p }),
  });
  const retention = useMutation({ mutationFn: api.runRetention, onSuccess: setStats });

  async function handleCheckUpdate() {
    setUpdateState({ kind: "checking" });
    try {
      const update = await check();
      if (update) {
        setUpdateState({ kind: "available", update });
      } else {
        setUpdateState({ kind: "up-to-date" });
      }
    } catch (e) {
      setUpdateState({ kind: "error", message: String(e) });
    }
  }

  async function handleInstallUpdate() {
    if (updateState.kind !== "available") return;
    const { update } = updateState;
    try {
      let downloaded = 0;
      let total = 0;
      setUpdateState({ kind: "downloading", progress: 0 });
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const pct = total > 0 ? Math.round((downloaded / total) * 100) : 0;
          setUpdateState({ kind: "downloading", progress: pct });
        } else if (event.event === "Finished") {
          setUpdateState({ kind: "ready" });
        }
      });
      setUpdateState({ kind: "ready" });
    } catch (e) {
      setUpdateState({ kind: "error", message: String(e) });
    }
  }

  if (!form) return <p className="text-sm text-muted-foreground">Loading…</p>;

  const update = (patch: Partial<AppSettings>) => setForm({ ...form, ...patch });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-bold">Settings</h1>
      </header>

      <Card>
        <CardHeader>
          <CardTitle>General</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-1.5">
            <Label>Default destination</Label>
            <div className="flex gap-2">
              <Input value={form.default_destination} readOnly placeholder="Choose…" />
              <Button variant="outline" onClick={() => pick.mutate()}>
                <FolderOpen className="h-4 w-4" /> Browse
              </Button>
            </div>
          </div>
          <div className="space-y-1.5">
            <Label>Theme</Label>
            <Select
              value={form.theme}
              onValueChange={(v) => {
                update({ theme: v });
                setTheme(v as "light" | "dark" | "system");
              }}
            >
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="system">System</SelectItem>
                <SelectItem value="light">Light</SelectItem>
                <SelectItem value="dark">Dark</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Google Workspace</CardTitle>
          <CardDescription>
            OAuth client for the Gmail provider (Google Cloud → Desktop app credentials).
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-1.5">
            <Label>Client ID</Label>
            <Input
              value={form.google_client_id}
              onChange={(e) => update({ google_client_id: e.target.value })}
              placeholder="…apps.googleusercontent.com"
            />
          </div>
          <div className="space-y-1.5">
            <Label>Client secret</Label>
            <Input
              type="password"
              value={form.google_client_secret}
              onChange={(e) => update({ google_client_secret: e.target.value })}
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Retention</CardTitle>
          <CardDescription>Keep the app from filling the disk over time.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <Label>Log retention (days)</Label>
              <Input
                type="number"
                value={form.log_retention_days}
                onChange={(e) => update({ log_retention_days: parseInt(e.target.value || "14", 10) })}
              />
            </div>
            <div className="space-y-1.5">
              <Label>Report history (count)</Label>
              <Input
                type="number"
                value={form.report_retention_count}
                onChange={(e) =>
                  update({ report_retention_count: parseInt(e.target.value || "50", 10) })
                }
              />
            </div>
          </div>
          <Button variant="outline" onClick={() => retention.mutate()} disabled={retention.isPending}>
            <Trash className="h-4 w-4" />
            {retention.isPending ? "Cleaning…" : "Run cleanup now"}
          </Button>
          {stats && (
            <div className="text-sm text-muted-foreground">
              Removed {stats.logs_deleted} logs, {stats.reports_deleted} reports,{" "}
              {stats.blobs_deleted} orphan blobs — reclaimed {formatBytes(stats.bytes_reclaimed)}.
            </div>
          )}
        </CardContent>
      </Card>

      {/* ── Application Updates ── */}
      <Card>
        <CardHeader>
          <CardTitle>Application Updates</CardTitle>
          <CardDescription>
            Current version: <span className="font-mono font-semibold">v{APP_VERSION}</span>
            {" · "}Updates are pulled from GitHub Releases and verified with a secure signature.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {updateState.kind === "idle" || updateState.kind === "error" ? (
            <>
              <Button
                variant="outline"
                onClick={handleCheckUpdate}
              >
                <RefreshCw className="h-4 w-4" />
                Check for Updates
              </Button>
              {updateState.kind === "error" && (
                <div className="flex items-center gap-2 text-sm text-destructive">
                  <AlertTriangle className="h-4 w-4 shrink-0" />
                  {updateState.message}
                </div>
              )}
            </>
          ) : updateState.kind === "checking" ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <RefreshCw className="h-4 w-4 animate-spin" />
              Checking for updates…
            </div>
          ) : updateState.kind === "up-to-date" ? (
            <div className="flex items-center gap-2 text-sm text-emerald-600">
              <CheckCircle2 className="h-4 w-4" />
              You're on the latest version (v{APP_VERSION}).
              <Button variant="ghost" size="sm" onClick={() => setUpdateState({ kind: "idle" })}>
                Check again
              </Button>
            </div>
          ) : updateState.kind === "available" ? (
            <div className="space-y-3">
              <div className="rounded-md border bg-muted/50 p-3 text-sm">
                <div className="font-semibold">
                  Update available: v{updateState.update.version}
                </div>
                {updateState.update.body && (
                  <p className="mt-1 whitespace-pre-wrap text-muted-foreground">
                    {updateState.update.body}
                  </p>
                )}
              </div>
              <Button onClick={handleInstallUpdate}>
                <Download className="h-4 w-4" />
                Download &amp; Install
              </Button>
            </div>
          ) : updateState.kind === "downloading" ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Download className="h-4 w-4 animate-bounce" />
                Downloading update{updateState.progress > 0 ? ` (${updateState.progress}%)` : "…"}
              </div>
              {updateState.progress > 0 && (
                <div className="h-2 w-full overflow-hidden rounded-full bg-secondary">
                  <div
                    className="h-full bg-primary transition-all"
                    style={{ width: `${updateState.progress}%` }}
                  />
                </div>
              )}
            </div>
          ) : (
            /* kind === "ready" */
            <div className="flex items-center gap-2 text-sm text-emerald-600">
              <CheckCircle2 className="h-4 w-4" />
              Update installed — please restart the app to apply it.
            </div>
          )}
        </CardContent>
      </Card>

      <div className="flex justify-end">
        <Button onClick={() => save.mutate(form)} disabled={save.isPending}>
          <Save className="h-4 w-4" />
          {save.isPending ? "Saving…" : "Save settings"}
        </Button>
      </div>
    </div>
  );
}
