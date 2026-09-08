import type * as React from "react";

import { LifeMessageDetails } from "../features/life-dock/LifeMessageDetails";

/** Product-specific receipts, rendered separately from model-authored prose. */
export function AppExtensionMessageDetails({
  authorPubkey,
  tags,
  children,
}: {
  authorPubkey?: string;
  tags?: string[][];
  children: React.ReactNode;
}) {
  return (
    <>
      {children}
      {authorPubkey && <LifeMessageDetails tags={tags ?? []} />}
    </>
  );
}
