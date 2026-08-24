import * as React from "react";

import { LazySettingsScreen } from "@/app/LazySettingsScreen";

type AppShellSettingsSurfaceProps = React.ComponentProps<
  typeof LazySettingsScreen
>;

export function AppShellSettingsSurface(props: AppShellSettingsSurfaceProps) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
      <React.Suspense fallback={null}>
        <LazySettingsScreen {...props} />
      </React.Suspense>
    </div>
  );
}
