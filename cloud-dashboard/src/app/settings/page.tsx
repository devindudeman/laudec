"use client";

import { useConvexAuth } from "convex/react";
import { SettingsView } from "@/components/SettingsView";

export default function SettingsPage() {
  const { isAuthenticated } = useConvexAuth();

  if (!isAuthenticated) {
    return (
      <div className="flex items-center justify-center min-h-screen text-zinc-500">
        Please sign in.
      </div>
    );
  }

  return <SettingsView />;
}
