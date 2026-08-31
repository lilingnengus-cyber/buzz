import { BuzzMark } from "./BuzzMark";

/**
 * The Buzz bee mark with flapping wings. Geometry is identical to the static
 * {@link BuzzMark} (v8 final keyframe) — the same silhouette, rendered in
 * `currentColor` so it tints per-theme — with the wing-flap keyframes (ported
 * from the Buzz website) beating the wings on an infinite loop.
 *
 * Unlike the static mark's single `<svg>`, each wing here is its own
 * HTML-level `<svg>` layer and the flap animates those elements' CSS
 * transforms. This is deliberate: WebKit paints SVG *children* on the main
 * thread, so a transform animation on a `<circle>` freezes for as long as boot
 * work (bundle eval, first React render of the app tree) hogs the thread —
 * exactly the window in which the loading gate is on screen. Transforms on
 * HTML-level elements run on the compositor (Core Animation in WKWebView) and
 * keep flapping regardless. The `bee-wing-layer` masks reproduce the slot
 * cutouts over the wings so the layered build stays pixel-identical to the
 * masked single-SVG mark (see animations.css).
 *
 * Everything is plain SVG + CSS (no JS/SMIL), so it paints on the very first
 * frame and the flap starts as soon as styles load. Reduced motion falls back
 * to the static silhouette via the CSS media query.
 */
export function FlappingBee({ className }: { className?: string }) {
  return (
    <BuzzMark
      className={["buzz-logo--scale-pulse", className]
        .filter(Boolean)
        .join(" ")}
    />
  );
}
