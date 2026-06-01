import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileDown, FolderOpen, FilePlus2, X, Loader2, CheckCircle2, AlertTriangle } from "lucide-react";
import { api, onExtractDone, onExtractProgress } from "@/lib/ipc";
import type { ExtractProgress, ExtractReport } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

function baseName(p: string): string {
  const parts = p.split(/[\\/]/);
  return parts[parts.length - 1] || p;
}

export function ExtractPage() {
  const { data: allExts = [] } = useQuery({
    queryKey: ["default-exts"],
    queryFn: api.defaultExtensions,
  });

  const [sources, setSources] = useState<string[]>([]);
  const [exts, setExts] = useState<string[]>([]);
  const [destination, setDestination] = useState("");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ExtractProgress | null>(null);
  const [report, setReport] = useState<ExtractReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const unsubs = [
      onExtractProgress((p) => setProgress(p)),
      onExtractDone(() => setRunning(false)),
    ];
    return () => unsubs.forEach((u) => u.then((f) => f()));
  }, []);

  const addFiles = async () => {
    const picked = await api.pickArchiveFiles();
    if (picked?.length) setSources((cur) => Array.from(new Set([...cur, ...picked])));
  };
  const addFolder = async () => {
    const folder = await api.pickFolder();
    if (folder) setSources((cur) => Array.from(new Set([...cur, folder])));
  };
  const removeSource = (s: string) => setSources((cur) => cur.filter((x) => x !== s));
  const toggleExt = (e: string) =>
    setExts((cur) => (cur.includes(e) ? cur.filter((x) => x !== e) : [...cur, e]));

  const run = async () => {
    setError(null);
    setReport(null);
    setProgress(null);
    setRunning(true);
    try {
      const r = await api.extractAttachments(sources, destination, exts);
      setReport(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  const canRun = sources.length > 0 && destination && !running;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-bold">Extract Attachments</h1>
        <p className="text-sm text-muted-foreground">
          Pull attachments out of existing <strong>EML, MBOX, MSG and PST</strong> files, filtered by
          type. Each source gets its own output subfolder.
        </p>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
          {error}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Source archives</CardTitle>
          <CardDescription>Add individual files or a folder (scanned recursively).</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex gap-2">
            <Button variant="outline" onClick={addFiles}>
              <FilePlus2 className="h-4 w-4" /> Add files
            </Button>
            <Button variant="outline" onClick={addFolder}>
              <FolderOpen className="h-4 w-4" /> Add folder
            </Button>
            {sources.length > 0 && (
              <Button variant="ghost" onClick={() => setSources([])}>
                Clear
              </Button>
            )}
          </div>
          {sources.length === 0 ? (
            <p className="text-sm text-muted-foreground">No sources selected.</p>
          ) : (
            <ul className="divide-y rounded-md border">
              {sources.map((s) => (
                <li key={s} className="flex items-center justify-between px-3 py-2 text-sm">
                  <span className="truncate" title={s}>
                    {baseName(s)}
                    <span className="ml-2 text-xs text-muted-foreground">{s}</span>
                  </span>
                  <button onClick={() => removeSource(s)} className="text-muted-foreground hover:text-destructive">
                    <X className="h-4 w-4" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Attachment types</CardTitle>
          <CardDescription>Leave empty to extract every attachment.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
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
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Destination</CardTitle>
        </CardHeader>
        <CardContent className="flex gap-2">
          <Input value={destination} readOnly placeholder="Choose an output folder…" />
          <Button variant="outline" onClick={() => api.pickFolder().then((p) => p && setDestination(p))}>
            <FolderOpen className="h-4 w-4" /> Browse
          </Button>
        </CardContent>
      </Card>

      <div className="flex items-center justify-between">
        {running && progress ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {progress.files_processed} file(s) · {progress.attachments_extracted} attachment(s) — {baseName(progress.file)}
          </div>
        ) : (
          <span />
        )}
        <Button size="lg" disabled={!canRun} onClick={run}>
          <FileDown className="h-4 w-4" />
          {running ? "Extracting…" : "Extract"}
        </Button>
      </div>

      {report && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <CheckCircle2 className="h-5 w-5 text-emerald-600" /> Done
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-sm">
            <div className="flex gap-6">
              <Stat label="Files" value={report.files_processed} />
              <Stat label="Attachments" value={report.attachments_extracted} />
              <Stat label="Skipped (filtered)" value={report.skipped_filtered} />
              <Stat label="Errors" value={report.errors.length} />
            </div>
            <div className="text-muted-foreground">Output: {report.output_root}</div>
            {report.errors.length > 0 && (
              <div className="space-y-1 rounded-md border border-amber-500/40 bg-amber-500/10 p-3">
                <div className="flex items-center gap-1 text-amber-700">
                  <AlertTriangle className="h-4 w-4" /> {report.errors.length} issue(s)
                </div>
                <ul className="max-h-40 overflow-y-auto text-xs text-muted-foreground">
                  {report.errors.map((e, i) => (
                    <li key={i}>{e}</li>
                  ))}
                </ul>
              </div>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div>
      <div className="text-lg font-semibold">{value}</div>
      <div className="text-xs text-muted-foreground">{label}</div>
    </div>
  );
}
