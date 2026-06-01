import { useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery } from "@tanstack/react-query";
import { FolderOpen, Play } from "lucide-react";
import { api } from "@/lib/ipc";
import type { BackupConfig, ExportFormat } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";

const FORMATS: { id: ExportFormat; label: string; note: string }[] = [
  { id: "eml", label: "EML", note: "one file per message" },
  { id: "mbox", label: "MBOX", note: "single mailbox file" },
  { id: "msg", label: "MSG", note: "Outlook message" },
  { id: "pst", label: "PST", note: "needs sidecar" },
];

function toEpoch(date: string): number | null {
  if (!date) return null;
  const t = Date.parse(date);
  return Number.isNaN(t) ? null : Math.floor(t / 1000);
}

export function BackupPage() {
  const navigate = useNavigate();
  const { data: accounts = [] } = useQuery({ queryKey: ["accounts"], queryFn: api.listAccounts });
  const { data: allExts = [] } = useQuery({
    queryKey: ["default-exts"],
    queryFn: api.defaultExtensions,
  });

  const [accountId, setAccountId] = useState<string>("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [subject, setSubject] = useState("");
  const [since, setSince] = useState("");
  const [before, setBefore] = useState("");
  const [formats, setFormats] = useState<ExportFormat[]>(["eml", "mbox"]);
  const [exts, setExts] = useState<string[]>([]);
  const [destination, setDestination] = useState("");
  const [downloadAtt, setDownloadAtt] = useState(true);
  const [keepRaw, setKeepRaw] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const toggleFormat = (f: ExportFormat) =>
    setFormats((cur) => (cur.includes(f) ? cur.filter((x) => x !== f) : [...cur, f]));
  const toggleExt = (e: string) =>
    setExts((cur) => (cur.includes(e) ? cur.filter((x) => x !== e) : [...cur, e]));

  const pick = useMutation({
    mutationFn: api.pickFolder,
    onSuccess: (p) => p && setDestination(p),
  });

  const start = useMutation({
    mutationFn: () => {
      const config: BackupConfig = {
        account_id: accountId,
        filter: {
          from: from || null,
          to: to || null,
          subject: subject || null,
          since: toEpoch(since),
          before: toEpoch(before),
          has_attachment: downloadAtt && exts.length > 0 ? true : null,
          extensions: exts,
        },
        formats,
        destination,
        download_attachments: downloadAtt,
        keep_raw: keepRaw,
      };
      return api.startBackup(config);
    },
    onSuccess: () => navigate("/jobs"),
    onError: (e: unknown) => setError(String(e)),
  });

  const canStart = accountId && destination && formats.length > 0 && !start.isPending;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-bold">New Backup</h1>
        <p className="text-sm text-muted-foreground">
          Filter messages, choose export formats and attachment types, then run a streaming backup.
        </p>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
          {error}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Source</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-1.5">
            <Label>Account</Label>
            <Select value={accountId} onValueChange={setAccountId}>
              <SelectTrigger>
                <SelectValue placeholder="Select an account" />
              </SelectTrigger>
              <SelectContent>
                {accounts.map((a) => (
                  <SelectItem key={a.id} value={a.id}>
                    {a.email} ({a.provider})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Filters</CardTitle>
        </CardHeader>
        <CardContent className="grid grid-cols-2 gap-4">
          <Field label="From"><Input value={from} onChange={(e) => setFrom(e.target.value)} placeholder="sender@…" /></Field>
          <Field label="To"><Input value={to} onChange={(e) => setTo(e.target.value)} placeholder="recipient@…" /></Field>
          <Field label="Subject"><Input value={subject} onChange={(e) => setSubject(e.target.value)} /></Field>
          <div />
          <Field label="Since"><Input type="date" value={since} onChange={(e) => setSince(e.target.value)} /></Field>
          <Field label="Before"><Input type="date" value={before} onChange={(e) => setBefore(e.target.value)} /></Field>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Export formats</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          {FORMATS.map((f) => (
            <button
              key={f.id}
              onClick={() => toggleFormat(f.id)}
              className={cn(
                "rounded-lg border px-4 py-2 text-left transition-colors",
                formats.includes(f.id)
                  ? "border-primary bg-primary/10"
                  : "hover:bg-accent"
              )}
            >
              <div className="text-sm font-semibold">{f.label}</div>
              <div className="text-xs text-muted-foreground">{f.note}</div>
            </button>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Attachments</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={downloadAtt} onCheckedChange={(v) => setDownloadAtt(!!v)} />
            Download attachments
          </label>
          {downloadAtt && (
            <div className="flex flex-wrap gap-2">
              {allExts.map((e) => (
                <button
                  key={e}
                  onClick={() => toggleExt(e)}
                  className={cn(
                    "rounded-full border px-3 py-1 text-xs uppercase transition-colors",
                    exts.includes(e) ? "border-primary bg-primary/10" : "hover:bg-accent"
                  )}
                >
                  {e}
                </button>
              ))}
              <span className="self-center text-xs text-muted-foreground">
                {exts.length === 0 ? "(all types)" : `${exts.length} selected`}
              </span>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Destination</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex gap-2">
            <Input value={destination} readOnly placeholder="Choose a folder…" />
            <Button variant="outline" onClick={() => pick.mutate()}>
              <FolderOpen className="h-4 w-4" /> Browse
            </Button>
          </div>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={keepRaw} onCheckedChange={(v) => setKeepRaw(!!v)} />
            Keep raw .eml in content-addressed store (enables dedupe + verification)
          </label>
        </CardContent>
      </Card>

      <div className="flex justify-end">
        <Button size="lg" disabled={!canStart} onClick={() => start.mutate()}>
          <Play className="h-4 w-4" />
          {start.isPending ? "Starting…" : "Start backup"}
        </Button>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
    </div>
  );
}
