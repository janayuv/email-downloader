import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Paperclip, Search } from "lucide-react";
import { api } from "@/lib/ipc";
import type { Filter, MessageHit } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { formatBytes } from "@/lib/utils";

export function BrowsePage() {
  const { data: accounts = [] } = useQuery({ queryKey: ["accounts"], queryFn: api.listAccounts });
  const [accountId, setAccountId] = useState<string>("all");
  const [text, setText] = useState("");
  const [from, setFrom] = useState("");
  const [subject, setSubject] = useState("");
  const [hasAtt, setHasAtt] = useState(false);
  const [results, setResults] = useState<MessageHit[]>([]);

  const run = useMutation({
    mutationFn: () => {
      const filter: Filter = {
        text: text || null,
        from: from || null,
        subject: subject || null,
        has_attachment: hasAtt ? true : null,
        extensions: [],
      };
      return api.searchMessages(filter, accountId === "all" ? undefined : accountId, 200);
    },
    onSuccess: setResults,
  });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-bold">Browse &amp; Search</h1>
        <p className="text-sm text-muted-foreground">
          Full-text search over the local FTS5 index of backed-up messages.
        </p>
      </header>

      <Card>
        <CardContent className="grid grid-cols-2 gap-4 py-6">
          <div className="space-y-1.5">
            <Label>Account</Label>
            <Select value={accountId} onValueChange={setAccountId}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All accounts</SelectItem>
                {accounts.map((a) => (
                  <SelectItem key={a.id} value={a.id}>
                    {a.email}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label>Text</Label>
            <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="any words…" />
          </div>
          <div className="space-y-1.5">
            <Label>From</Label>
            <Input value={from} onChange={(e) => setFrom(e.target.value)} />
          </div>
          <div className="space-y-1.5">
            <Label>Subject</Label>
            <Input value={subject} onChange={(e) => setSubject(e.target.value)} />
          </div>
          <label className="col-span-2 flex items-center gap-2 text-sm">
            <Checkbox checked={hasAtt} onCheckedChange={(v) => setHasAtt(!!v)} />
            Has attachments
          </label>
          <div className="col-span-2">
            <Button onClick={() => run.mutate()} disabled={run.isPending}>
              <Search className="h-4 w-4" />
              {run.isPending ? "Searching…" : "Search"}
            </Button>
          </div>
        </CardContent>
      </Card>

      <div className="text-sm text-muted-foreground">{results.length} result(s)</div>
      <div className="overflow-hidden rounded-lg border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr>
              <th className="px-4 py-2">Subject</th>
              <th className="px-4 py-2">From</th>
              <th className="px-4 py-2">Date</th>
              <th className="px-4 py-2 text-right">Size</th>
            </tr>
          </thead>
          <tbody>
            {results.map((r) => (
              <tr key={r.id} className="border-t hover:bg-accent/40">
                <td className="px-4 py-2">
                  <div className="flex items-center gap-2">
                    {r.has_attachments && <Paperclip className="h-3 w-3 text-muted-foreground" />}
                    <span className="truncate">{r.subject || "(no subject)"}</span>
                  </div>
                </td>
                <td className="px-4 py-2 text-muted-foreground">{r.from_addr}</td>
                <td className="px-4 py-2 text-muted-foreground">
                  {r.internal_date ? new Date(r.internal_date * 1000).toLocaleDateString() : "—"}
                </td>
                <td className="px-4 py-2 text-right text-muted-foreground">{formatBytes(r.size)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
