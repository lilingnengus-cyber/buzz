export const LIFE_DOCK_PREFERENCES_KEY = "buzz.life-dock.preferences.v1";

export type LifeDockPreferences = {
  followConversation: boolean;
  pinned: boolean;
};

export const DEFAULT_LIFE_DOCK_PREFERENCES: LifeDockPreferences = {
  followConversation: true,
  pinned: false,
};

type StorageLike = Pick<Storage, "getItem" | "setItem">;

export function readLifeDockPreferences(
  storage: StorageLike | null | undefined,
): LifeDockPreferences {
  if (!storage) return DEFAULT_LIFE_DOCK_PREFERENCES;
  try {
    const value = JSON.parse(
      storage.getItem(LIFE_DOCK_PREFERENCES_KEY) ?? "null",
    );
    return {
      followConversation:
        typeof value?.followConversation === "boolean"
          ? value.followConversation
          : true,
      pinned: typeof value?.pinned === "boolean" ? value.pinned : false,
    };
  } catch {
    return DEFAULT_LIFE_DOCK_PREFERENCES;
  }
}

export function saveLifeDockPreferences(
  storage: StorageLike | null | undefined,
  preferences: LifeDockPreferences,
): void {
  if (!storage) return;
  try {
    storage.setItem(LIFE_DOCK_PREFERENCES_KEY, JSON.stringify(preferences));
  } catch {
    // Keep the in-memory Life preference when storage is unavailable.
  }
}
