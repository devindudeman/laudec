"use client";

import { useQuery, useMutation } from "convex/react";
import { api } from "../../convex/_generated/api";
import { Id } from "../../convex/_generated/dataModel";
import { useState } from "react";
import Link from "next/link";

export function SettingsView() {
  const teams = useQuery(api.teams.list);
  const [selectedTeamId, setSelectedTeamId] = useState<Id<"teams"> | null>(null);

  if (teams === undefined) {
    return (
      <div className="flex items-center justify-center min-h-screen text-zinc-500">
        Loading...
      </div>
    );
  }

  const activeTeamId = selectedTeamId ?? teams[0]?._id ?? null;

  return (
    <div className="min-h-screen flex flex-col">
      <header className="border-b border-zinc-800 bg-zinc-900/50 px-6 py-3 flex items-center justify-between">
        <div className="flex items-center gap-4">
          <Link href="/" className="text-zinc-400 hover:text-zinc-200 text-sm">
            ← SESSIONS
          </Link>
          <h1 className="text-lg font-bold tracking-tight">Settings</h1>
          {teams.length > 1 && (
            <select
              value={activeTeamId ?? ""}
              onChange={(e) => setSelectedTeamId(e.target.value as Id<"teams">)}
              className="bg-zinc-800 border border-zinc-700 rounded-md px-3 py-1 text-sm"
            >
              {teams.map((t) => (
                <option key={t._id} value={t._id}>
                  {t.name}
                </option>
              ))}
            </select>
          )}
        </div>
      </header>

      <main className="flex-1 p-6 max-w-[900px] mx-auto w-full space-y-8">
        {activeTeamId && (
          <>
            <TeamInfo teamId={activeTeamId} teams={teams} />
            <ApiKeysSection teamId={activeTeamId} />
            <SetupGuide teamId={activeTeamId} />
          </>
        )}
      </main>
    </div>
  );
}

function TeamInfo({
  teamId,
  teams,
}: {
  teamId: Id<"teams">;
  teams: { _id: Id<"teams">; name: string; role: string }[];
}) {
  const team = teams.find((t) => t._id === teamId);
  const members = useQuery(api.teams.getMembers, { teamId });

  return (
    <section>
      <SectionTitle>Team</SectionTitle>
      <div className="bg-zinc-900 border border-zinc-800 p-4">
        <div className="flex items-center gap-4 mb-3">
          <span className="text-lg font-bold">{team?.name ?? "—"}</span>
          <span className="text-[10px] uppercase tracking-wider text-zinc-500 border border-zinc-700 px-2 py-0.5">
            {team?.role ?? "member"}
          </span>
        </div>
        {members && (
          <div className="text-sm text-zinc-400">
            {members.length} member{members.length !== 1 ? "s" : ""}
          </div>
        )}
      </div>
    </section>
  );
}

function ApiKeysSection({ teamId }: { teamId: Id<"teams"> }) {
  const keys = useQuery(api.apiKeys.list, { teamId });
  const createKey = useMutation(api.apiKeys.create);
  const revokeKey = useMutation(api.apiKeys.revoke);
  const [newKeyName, setNewKeyName] = useState("");
  const [creating, setCreating] = useState(false);
  const [justCreated, setJustCreated] = useState<{
    key: string;
    keyId: string;
  } | null>(null);
  const [copied, setCopied] = useState(false);

  async function handleCreate() {
    if (!newKeyName.trim()) return;
    setCreating(true);
    try {
      const result = await createKey({ teamId, name: newKeyName.trim() });
      setJustCreated({ key: result.key, keyId: result.keyId });
      setNewKeyName("");
    } finally {
      setCreating(false);
    }
  }

  async function handleCopy(text: string) {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  async function handleRevoke(keyId: Id<"apiKeys">) {
    if (!confirm("Revoke this API key? This cannot be undone.")) return;
    await revokeKey({ keyId });
  }

  return (
    <section>
      <SectionTitle>API Keys</SectionTitle>
      <p className="text-sm text-zinc-500 mb-4">
        API keys authenticate laudec when pushing session data to this dashboard.
      </p>

      {/* Just-created key warning */}
      {justCreated && (
        <div className="bg-amber-950/30 border border-amber-700 p-4 mb-4">
          <div className="text-sm font-semibold text-amber-400 mb-2">
            ⚠️ Copy your API key now — it won&apos;t be shown again
          </div>
          <div className="flex items-center gap-2">
            <code className="flex-1 bg-zinc-950 text-emerald-400 px-3 py-2 text-sm font-mono break-all">
              {justCreated.key}
            </code>
            <button
              onClick={() => handleCopy(justCreated.key)}
              className="shrink-0 bg-zinc-700 hover:bg-zinc-600 px-3 py-2 text-sm font-medium"
            >
              {copied ? "Copied!" : "Copy"}
            </button>
          </div>
          <button
            onClick={() => setJustCreated(null)}
            className="text-xs text-zinc-500 hover:text-zinc-300 mt-2"
          >
            Dismiss
          </button>
        </div>
      )}

      {/* Create new key */}
      <div className="flex gap-2 mb-4">
        <input
          type="text"
          value={newKeyName}
          onChange={(e) => setNewKeyName(e.target.value)}
          placeholder="Key name (e.g. my-laptop, ci-server)"
          className="flex-1 bg-zinc-900 border border-zinc-700 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          onKeyDown={(e) => e.key === "Enter" && handleCreate()}
        />
        <button
          onClick={handleCreate}
          disabled={!newKeyName.trim() || creating}
          className="bg-blue-600 hover:bg-blue-500 disabled:opacity-50 rounded-md px-4 py-2 text-sm font-medium transition-colors shrink-0"
        >
          {creating ? "Creating..." : "Create key"}
        </button>
      </div>

      {/* Existing keys */}
      {keys === undefined ? (
        <div className="text-zinc-500 text-sm">Loading keys...</div>
      ) : keys.length === 0 ? (
        <div className="text-zinc-500 text-sm py-4 text-center border border-zinc-800 bg-zinc-900">
          No API keys yet
        </div>
      ) : (
        <table className="w-full border border-zinc-700 border-collapse text-[12px]">
          <thead>
            <tr className="bg-zinc-100 text-zinc-900">
              <Th>Name</Th>
              <Th>Key</Th>
              <Th>Created</Th>
              <Th>Last Used</Th>
              <Th>Status</Th>
              <Th align="right">Actions</Th>
            </tr>
          </thead>
          <tbody>
            {keys.map((k) => {
              const isRevoked = k.revokedAt != null;
              return (
                <tr
                  key={k._id}
                  className={`border-b border-zinc-800 ${isRevoked ? "opacity-50" : ""}`}
                >
                  <td className="px-2.5 py-1.5 font-medium">{k.name}</td>
                  <td className="px-2.5 py-1.5 font-mono text-zinc-500">
                    {k.keyPrefix}...
                  </td>
                  <td className="px-2.5 py-1.5 text-zinc-500 whitespace-nowrap">
                    {new Date(k.createdAt).toLocaleDateString()}
                  </td>
                  <td className="px-2.5 py-1.5 text-zinc-500 whitespace-nowrap">
                    {k.lastUsedAt
                      ? new Date(k.lastUsedAt).toLocaleDateString()
                      : "Never"}
                  </td>
                  <td className="px-2.5 py-1.5">
                    {isRevoked ? (
                      <span className="text-red-400 text-[10px] font-bold uppercase">
                        Revoked
                      </span>
                    ) : (
                      <span className="text-emerald-400 text-[10px] font-bold uppercase">
                        Active
                      </span>
                    )}
                  </td>
                  <td className="px-2.5 py-1.5 text-right">
                    {!isRevoked && (
                      <button
                        onClick={() => handleRevoke(k._id)}
                        className="text-red-400 hover:text-red-300 text-[11px] font-semibold"
                      >
                        Revoke
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}

function SetupGuide({ teamId }: { teamId: Id<"teams"> }) {
  return (
    <section>
      <SectionTitle>Setup Guide</SectionTitle>
      <div className="bg-zinc-900 border border-zinc-800 p-4 space-y-4 text-sm">
        <p className="text-zinc-400">
          Add the following to your project&apos;s <code className="text-emerald-400 bg-zinc-800 px-1.5 py-0.5 text-xs">laudec.toml</code>:
        </p>
        <pre className="bg-zinc-950 text-emerald-400 px-4 py-3 text-xs overflow-x-auto">
{`[cloud]
enabled = true
endpoint = "${typeof window !== "undefined" ? window.location.origin.replace("3000", "convex.site").replace("http://100.96.154.24", "https://acoustic-basilisk-685.convex.site") : "https://your-project.convex.site"}"
api_key = "ldc_..."  # paste your API key here`}
        </pre>
        <p className="text-zinc-400">
          Then run <code className="text-emerald-400 bg-zinc-800 px-1.5 py-0.5 text-xs">laudec .</code> in
          your project directory. Sessions will appear in the dashboard in
          real-time.
        </p>
      </div>
    </section>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[11px] font-bold uppercase tracking-wider border-b border-zinc-800 pb-1 mb-3">
      {children}
    </div>
  );
}

function Th({
  children,
  align = "left",
}: {
  children: React.ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={`px-2.5 py-1.5 font-semibold uppercase text-[10px] tracking-wider whitespace-nowrap ${
        align === "right" ? "text-right" : "text-left"
      }`}
    >
      {children}
    </th>
  );
}
