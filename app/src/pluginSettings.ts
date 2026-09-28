import type { PluginSettings, Setting, SettingValue } from "./types";

/** A plugin's settings with one value changed. A value back at its default is left out, so settings.toml only holds what was changed. */
export function withValue(own: PluginSettings, setting: Setting, value: SettingValue): PluginSettings {
  const { [setting.key]: _, ...rest } = own;
  const next = value === setting.default ? rest : { ...rest, [setting.key]: value };
  return next as PluginSettings;
}

/** A setting's description, and for numbers the range they may take. */
export function hint(setting: Setting): string | undefined {
  const parts = [setting.description];
  if (setting.type === "number") {
    const { min, max } = setting;
    if (min !== null && max !== null) parts.push(`${min} to ${max}`);
    else if (min !== null) parts.push(`${min} or more`);
    else if (max !== null) parts.push(`${max} or less`);
  }
  return parts.filter(Boolean).join(" · ") || undefined;
}
