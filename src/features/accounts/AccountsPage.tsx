import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Mail, Plus, Trash2, ShieldCheck } from "lucide-react";
import { api } from "@/lib/ipc";
import type { Account } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";

export function AccountsPage() {
  const qc = useQueryClient();
  const { data: accounts = [], isLoading } = useQuery({
    queryKey: ["accounts"],
    queryFn: api.listAccounts,
  });
  const [showImap, setShowImap] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const del = useMutation({
    mutationFn: (id: string) => api.deleteAccount(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["accounts"] }),
  });

  const addGmail = useMutation({
    mutationFn: () => api.addGmailAccount(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["accounts"] }),
    onError: (e: unknown) => setError(String(e)),
  });

  return (
    <div className="space-y-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">Accounts</h1>
          <p className="text-sm text-muted-foreground">
            Connect Gmail / Google Workspace or any IMAP mailbox. Credentials are stored in
            the OS keychain — never in the database.
          </p>
        </div>
        <div className="flex gap-2">
          <Button onClick={() => addGmail.mutate()} disabled={addGmail.isPending}>
            <Mail className="h-4 w-4" />
            {addGmail.isPending ? "Authorizing…" : "Add Gmail"}
          </Button>
          <Button variant="outline" onClick={() => setShowImap((v) => !v)}>
            <Plus className="h-4 w-4" /> Add IMAP
          </Button>
        </div>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
          {error}
        </div>
      )}

      {showImap && (
        <ImapForm
          onDone={() => {
            setShowImap(false);
            qc.invalidateQueries({ queryKey: ["accounts"] });
          }}
          onError={setError}
        />
      )}

      {isLoading ? (
        <p className="text-sm text-muted-foreground">Loading…</p>
      ) : accounts.length === 0 ? (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            No accounts yet. Add a Gmail or IMAP account to get started.
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-3">
          {accounts.map((a) => (
            <AccountRow key={a.id} account={a} onDelete={() => del.mutate(a.id)} />
          ))}
        </div>
      )}
    </div>
  );
}

function AccountRow({ account, onDelete }: { account: Account; onDelete: () => void }) {
  return (
    <Card>
      <CardContent className="flex items-center justify-between py-4">
        <div className="flex items-center gap-3">
          <div className="rounded-md bg-primary/10 p-2">
            <Mail className="h-5 w-5 text-primary" />
          </div>
          <div>
            <div className="font-medium">{account.email}</div>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <Badge variant={account.provider === "gmail" ? "default" : "secondary"}>
                {account.provider.toUpperCase()}
              </Badge>
              {account.imap_host && <span>{account.imap_host}:{account.imap_port}</span>}
              <span className="flex items-center gap-1">
                <ShieldCheck className="h-3 w-3" /> keychain
              </span>
            </div>
          </div>
        </div>
        <Button variant="ghost" size="icon" onClick={onDelete}>
          <Trash2 className="h-4 w-4 text-destructive" />
        </Button>
      </CardContent>
    </Card>
  );
}

function ImapForm({
  onDone,
  onError,
}: {
  onDone: () => void;
  onError: (e: string) => void;
}) {
  const [email, setEmail] = useState("");
  const [host, setHost] = useState("imap.gmail.com");
  const [port, setPort] = useState(993);
  const [password, setPassword] = useState("");

  const add = useMutation({
    mutationFn: () => api.addImapAccount(email, host, port, password),
    onSuccess: onDone,
    onError: (e: unknown) => onError(String(e)),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Add IMAP account</CardTitle>
      </CardHeader>
      <CardContent className="grid grid-cols-2 gap-4">
        <div className="space-y-1.5">
          <Label>Email</Label>
          <Input value={email} onChange={(e) => setEmail(e.target.value)} placeholder="you@example.com" />
        </div>
        <div className="space-y-1.5">
          <Label>Password / app password</Label>
          <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </div>
        <div className="space-y-1.5">
          <Label>IMAP host</Label>
          <Input value={host} onChange={(e) => setHost(e.target.value)} />
        </div>
        <div className="space-y-1.5">
          <Label>Port</Label>
          <Input
            type="number"
            value={port}
            onChange={(e) => setPort(parseInt(e.target.value || "993", 10))}
          />
        </div>
        <div className="col-span-2">
          <Button onClick={() => add.mutate()} disabled={add.isPending || !email || !password}>
            {add.isPending ? "Verifying…" : "Connect & save"}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
