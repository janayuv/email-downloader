import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Save, Trash, FolderOpen } from "lucide-react";
import { api } from "@/lib/ipc";
import type { AppSettings, RetentionStats } from "@/lib/types";
import { useAppStore } from "@/stores/appStore";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { formatBytes } from "@/lib/utils";

export function SettingsPage() {
  const qc = useQueryClient();
  const setTheme = useAppStore((s) => s.setTheme);
  const { data } = useQuery({ queryKey: ["settings"], queryFn: api.getSettings });
  const [form, setForm] = useState<AppSettings | null>(null);
  const [stats, setStats] = useState<RetentionStats | null>(null);

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

      <div className="flex justify-end">
        <Button onClick={() => save.mutate(form)} disabled={save.isPending}>
          <Save className="h-4 w-4" />
          {save.isPending ? "Saving…" : "Save settings"}
        </Button>
      </div>
    </div>
  );
}
