import { WorkspaceDockHost } from "@/features/workspace-dock";

/** Product-specific dock kept behind one stable Buzz shell integration point. */
export function AppExtensionDock() {
  return <WorkspaceDockHost />;
}
