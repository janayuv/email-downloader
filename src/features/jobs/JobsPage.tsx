import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Ban, CheckCircle2, Loader2, XCircle, Clock } from "lucide-react";
import { api, onJobDone, onJobError, onJobProgress } from "@/lib/ipc";
import type { Job, JobProgress, JobStatus } from "@/lib/types";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";

const statusMeta: Record<JobStatus, { icon: typeof Clock; variant: "default" | "secondary" | "destructive" | "success" | "warning"; label: string }> = {
  pending: { icon: Clock, variant: "secondary", label: "Pending" },
  running: { icon: Loader2, variant: "default", label: "Running" },
  completed: { icon: CheckCircle2, variant: "success", label: "Completed" },
  failed: { icon: XCircle, variant: "destructive", label: "Failed" },
  cancelled: { icon: Ban, variant: "warning", label: "Cancelled" },
};

export function JobsPage() {
  const qc = useQueryClient();
  const { data: jobs = [] } = useQuery({
    queryKey: ["jobs"],
    queryFn: () => api.listJobs(100),
    refetchInterval: 4000,
  });
  const [live, setLive] = useState<Record<string, JobProgress>>({});

  useEffect(() => {
    const unsubs: Array<Promise<() => void>> = [
      onJobProgress((p) => setLive((cur) => ({ ...cur, [p.job_id]: p }))),
      onJobDone(() => qc.invalidateQueries({ queryKey: ["jobs"] })),
      onJobError(() => qc.invalidateQueries({ queryKey: ["jobs"] })),
    ];
    return () => {
      unsubs.forEach((u) => u.then((f) => f()));
    };
  }, [qc]);

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-bold">Jobs</h1>
        <p className="text-sm text-muted-foreground">
          Live backup progress. Jobs resume automatically after a crash (idempotent via
          content hashing).
        </p>
      </header>

      {jobs.length === 0 ? (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            No jobs yet. Start a backup to see progress here.
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {jobs.map((j) => (
            <JobRow key={j.id} job={j} live={live[j.id]} onCancel={() => api.cancelJob(j.id)} />
          ))}
        </div>
      )}
    </div>
  );
}

function JobRow({
  job,
  live,
  onCancel,
}: {
  job: Job;
  live?: JobProgress;
  onCancel: () => void;
}) {
  const status = (live?.status ?? job.status) as JobStatus;
  const meta = statusMeta[status];
  const Icon = meta.icon;
  const messages = live?.messages_done ?? job.messages_done;
  const attachments = live?.attachments_done ?? job.attachments_done;
  const failed = live?.failed ?? job.failed;
  const running = status === "running" || status === "pending";

  return (
    <Card>
      <CardContent className="space-y-3 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Badge variant={meta.variant} className="gap-1">
              <Icon className={`h-3 w-3 ${status === "running" ? "animate-spin" : ""}`} />
              {meta.label}
            </Badge>
            <span className="font-mono text-xs text-muted-foreground">{job.id.slice(0, 8)}</span>
          </div>
          {running && (
            <Button variant="outline" size="sm" onClick={onCancel}>
              <Ban className="h-4 w-4" /> Cancel
            </Button>
          )}
        </div>

        {running && <Progress value={undefined} className="animate-pulse" />}

        <div className="flex gap-6 text-sm">
          <Stat label="Messages" value={messages} />
          <Stat label="Attachments" value={attachments} />
          <Stat label="Failed" value={failed} tone={failed > 0 ? "bad" : undefined} />
          {live?.current && status === "running" && (
            <div className="ml-auto truncate text-xs text-muted-foreground">
              {live.current}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function Stat({ label, value, tone }: { label: string; value: number; tone?: "bad" }) {
  return (
    <div>
      <div className={`text-lg font-semibold ${tone === "bad" ? "text-destructive" : ""}`}>
        {value}
      </div>
      <div className="text-xs text-muted-foreground">{label}</div>
    </div>
  );
}
