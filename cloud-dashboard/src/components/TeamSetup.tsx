"use client";

import { useMutation } from "convex/react";
import { api } from "../../convex/_generated/api";
import { useState } from "react";

export function TeamSetup() {
  const createTeam = useMutation(api.teams.create);
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setCreating(true);
    try {
      await createTeam({ name: name.trim() });
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="flex items-center justify-center min-h-screen">
      <div className="bg-gray-900 border border-gray-800 rounded-lg p-8 max-w-sm w-full">
        <h2 className="text-xl font-bold mb-2">Create a team</h2>
        <p className="text-gray-400 text-sm mb-6">
          Teams group your Claude Code sessions and API keys.
        </p>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Team name"
          className="w-full bg-gray-800 border border-gray-700 rounded-md px-3 py-2 text-sm mb-4 focus:outline-none focus:ring-2 focus:ring-blue-500"
          onKeyDown={(e) => e.key === "Enter" && handleCreate()}
        />
        <button
          onClick={handleCreate}
          disabled={!name.trim() || creating}
          className="w-full bg-blue-600 hover:bg-blue-500 disabled:opacity-50 rounded-md px-4 py-2 text-sm font-medium transition-colors"
        >
          {creating ? "Creating..." : "Create team"}
        </button>
      </div>
    </div>
  );
}
