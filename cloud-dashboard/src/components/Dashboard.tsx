"use client";

import { useQuery } from "convex/react";
import { useAuthActions } from "@convex-dev/auth/react";
import { api } from "../../convex/_generated/api";
import { useState } from "react";
import { SessionsList } from "./SessionsList";
import { TeamSetup } from "./TeamSetup";
import { Id } from "../../convex/_generated/dataModel";
import Link from "next/link";

export function Dashboard() {
  const { signOut } = useAuthActions();
  const teams = useQuery(api.teams.list);
  const [selectedTeamId, setSelectedTeamId] = useState<Id<"teams"> | null>(
    null
  );

  if (teams === undefined) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-gray-400">Loading...</div>
      </div>
    );
  }

  // Auto-select first team
  const activeTeamId = selectedTeamId ?? teams[0]?._id ?? null;

  if (teams.length === 0) {
    return <TeamSetup />;
  }

  return (
    <div className="min-h-screen flex flex-col">
      {/* Header */}
      <header className="border-b border-gray-800 bg-gray-900/50 px-6 py-3 flex items-center justify-between">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-bold tracking-tight">laudec cloud</h1>
          <select
            value={activeTeamId ?? ""}
            onChange={(e) =>
              setSelectedTeamId(e.target.value as Id<"teams">)
            }
            className="bg-gray-800 border border-gray-700 rounded-md px-3 py-1 text-sm"
          >
            {teams.map((t) => (
              <option key={t._id} value={t._id}>
                {t.name}
              </option>
            ))}
          </select>
        </div>
        <div className="flex items-center gap-4">
          <Link
            href="/settings"
            className="text-zinc-400 hover:text-zinc-200 text-sm"
          >
            Settings
          </Link>
          <button
            onClick={() => void signOut()}
            className="text-zinc-400 hover:text-zinc-200 text-sm"
          >
            Sign out
          </button>
        </div>
      </header>

      {/* Main content */}
      <main className="flex-1 p-6">
        {activeTeamId && <SessionsList teamId={activeTeamId} />}
      </main>
    </div>
  );
}
