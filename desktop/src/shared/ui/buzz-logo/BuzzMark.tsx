import "./buzz-logo-animation.css";

/**
 * The Pacioli application mark. The component name stays stable internally so
 * existing feature imports do not need to know about the product rename.
 */
export function BuzzMark({ className }: { className?: string }) {
  return (
    <img
      alt=""
      aria-hidden="true"
      className={["buzz-mark", "buzz-logo__mark", "object-contain", className]
        .filter(Boolean)
        .join(" ")}
      draggable={false}
      src="/pacioli-logo.png"
    />
  );
}
