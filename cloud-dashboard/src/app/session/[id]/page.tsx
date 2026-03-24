"use client";

import { useParams } from "next/navigation";
import { SessionDetailView } from "@/components/SessionDetailView";
import { Id } from "../../../../convex/_generated/dataModel";

export default function SessionPage() {
  const params = useParams();
  const id = params.id as string;

  return <SessionDetailView id={id as Id<"sessions">} />;
}
