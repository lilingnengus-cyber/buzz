const WELCOME_GUIDE_PERSONA_ID = "builtin:fizz";

/** Prefers the built-in welcome lead for a new Projects conversation. */
export function pickDefaultProjectsAgent<
  Agent extends { name: string; personaId?: string | null },
>(agents: readonly Agent[]): Agent | null {
  return (
    agents.find((agent) => agent.personaId === WELCOME_GUIDE_PERSONA_ID) ??
    agents.find((agent) => {
      const name = agent.name.trim().toLocaleLowerCase();
      return name === "fizz" || name === "kit";
    }) ??
    agents[0] ??
    null
  );
}
