export interface Credential {
  id: string;
  connection_id: string;
  connection_type?: string;
  app_server?: string;
  system_number?: string;
  system_id?: string;
  message_server?: string;
  message_server_port?: string;
  logon_group?: string;
  saprouter?: string;
  description?: string;
  uuid?: string;
  client: string;
  username: string;
  password?: string;
  encrypted_password: string;
  language: string;
  post_login_action?: string;
  post_login_action_type?: string;
  group_id?: string;
  environment?: string;
  login_count?: number;
  last_login_at?: string;
  color_tag?: string;
  is_favorite?: boolean;
  favorite_order?: number;
  display_name?: string;
  snc_enabled?: boolean;
  snc_name?: string;
  snc_qop?: string;
  snc_sso?: boolean;
  created_at: string;
  updated_at: string;
}

export interface Group {
  id: string;
  group_name: string;
  entries: string[];
  is_system?: boolean;
  is_default?: boolean;
  created_at: string;
  updated_at: string;
}

export interface SapConnection {
  name: string;
  description?: string;
  server?: string;
  system_number?: string;
  system_id?: string;
  group?: string;
  service?: string;
  connection_type?: string;
  workspace_name?: string;
  saprouter?: string;
  message_server?: string;
  message_server_port?: string;
  logon_group?: string;
  uuid?: string;
  sncop?: string;
}

export interface AppSettings {
  auto_start: boolean;
  minimize_to_tray: boolean;
  close_to_tray: boolean;
  default_language: string;
  sap_logon_path?: string;
  theme: string;
  password_free: boolean;
  batch_login_interval: number;
  always_on_top: boolean;
  auto_lock_minutes: number;
  group_by_environment: boolean;
  compact_mode: boolean;
  clipboard_clear_seconds: number;
  default_group: string;
}

export const COLOR_TAGS = [
  { value: "", label: "color.none", color: "#9ca3af" },
  { value: "red", label: "color.red", color: "#da1e28" },
  { value: "orange", label: "color.orange", color: "#f97316" },
  { value: "yellow", label: "color.yellow", color: "#f1c21b" },
  { value: "green", label: "color.green", color: "#198038" },
  { value: "blue", label: "color.blue", color: "#0f62fe" },
  { value: "purple", label: "color.purple", color: "#7c3aed" },
];

export const ENV_COLORS: Record<string, string> = { production: "#da1e28", test: "#f1c21b", development: "#198038", configuration: "#8a3ffc" };
export const ENV_LABEL_KEYS: Record<string, string> = { production: "env.production", test: "env.test", development: "env.development", configuration: "env.configuration" };

export const THEMES = [
  { value: "light", label: "theme.light" },
  { value: "light-green", label: "theme.light-green" },
  { value: "light-warm", label: "theme.light-warm" },
  { value: "light-purple", label: "theme.light-purple" },
  { value: "dark", label: "theme.dark" },
  { value: "dark-green", label: "theme.dark-green" },
  { value: "dark-warm", label: "theme.dark-warm" },
  { value: "dark-purple", label: "theme.dark-purple" },
];
