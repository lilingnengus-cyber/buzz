import { copyTextToClipboard } from "../../shared/lib/clipboard";
import { lifeMessageReceipt } from "./lifeMessageReceipt";

/** Receipt for a known agent signer; values come only from validated event tags. */
export function LifeMessageDetails({ tags }: { tags: string[][] }) {
  const receipt = lifeMessageReceipt(tags);
  if (!receipt) return null;
  return (
    <details className="mt-2 text-xs text-muted-foreground">
      <summary className="w-fit cursor-pointer rounded focus-visible:outline focus-visible:outline-2">
        查看执行详情
      </summary>
      <pre className="mt-2 whitespace-pre-wrap break-all font-mono text-xs">
        {receipt}
      </pre>
      <button
        className="mt-2 rounded px-2 py-1 hover:bg-accent focus-visible:outline focus-visible:outline-2"
        type="button"
        onClick={() => copyTextToClipboard(receipt, "执行详情已复制")}
      >
        复制执行详情
      </button>
    </details>
  );
}
