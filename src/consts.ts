export const COFF_FROM_COMPRESSED_SIZE = 0.3;
export const DEFAULT_BIND_LTX = "default.ltx";
export const CUSTOM_BIND_LTX = "custom.ltx";

export enum Lang {
  Ru = 'ru',
  En = 'en',
}

export enum ConnectStatus {
  Connnecting = 'connnecting',
  Connnected = 'connnected',
  ConnnectError = 'connnect_error',
}

export enum DownloadStatus {
  Init = "Init",
  Pause = "Pause",
  DownloadFiles = "DownloadFiles",
  Unpacking = "Unpacking",
}

export enum LangType {
  Rus = "Rus",
  Eng = "Eng",
}

export enum RenderType {
  RendererR2 = "RendererR2",
  RendererR25 = "RendererR25",
  RendererR3 = "RendererR3",
  RendererR4 = "RendererR4",
  RendererRgl = "RendererRgl",
}

export const NO_KEY = "---";
export const KEYS_MAP: Record<string, string> = {
  // Мышь
  Mouse0: "mouse1", // Left
  Mouse1: "mouse3", // Middle (в SDL обычно 3)
  Mouse2: "mouse2", // Right
  Mouse3: "mouse4", // X1
  Mouse4: "mouse5", // X2

  // Буквы (KeyA -> kA)
  ...Object.fromEntries(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").map(char => [`Key${char}`, `k${char}`])
  ),

  // Цифры (Digit1 -> k1)
  ...Object.fromEntries(
    "1234567890".split("").map(char => [`Digit${char}`, `k${char}`])
  ),

  // Модификаторы
  ControlLeft: "kLCONTROL",
  ShiftLeft: "kLSHIFT",
  AltLeft: "kLMENU",
  MetaLeft: "kLWIN",
  ControlRight: "kRCONTROL",
  ShiftRight: "kRSHIFT",
  AltRight: "kRMENU",
  MetaRight: "kRWIN",

  // Навигация и управление
  Enter: "kRETURN",
  Escape: "kESCAPE",
  Backspace: "kBACK",
  Tab: "kTAB",
  Space: "kSPACE",
  CapsLock: "kCAPITAL",
  Insert: "kINSERT",
  Delete: "kDELETE",
  Home: "kHOME",
  End: "kEND",
  PageUp: "kPGUP",
  PageDown: "kPGDN",

  // Стрелки
  ArrowUp: "kUP",
  ArrowDown: "kDOWN",
  ArrowLeft: "kLEFT",
  ArrowRight: "kRIGHT",

  // Функциональные клавиши
  ...Object.fromEntries(
    Array.from({ length: 12 }, (_, i) => [`F${i + 1}`, `kF${i + 1}`])
  ),

  // Символы
  Minus: "kMINUS",
  Equal: "kEQUALS",
  BracketLeft: "kLBRACKET",
  BracketRight: "kRBRACKET",
  Backslash: "kBACKSLASH",
  Semicolon: "kSEMICOLON",
  Quote: "kAPOSTROPHE",
  Backquote: "kGRAVE",
  Comma: "kCOMMA",
  Period: "kPERIOD",
  Slash: "kSLASH",

  // Нампад
  NumLock: "kNUMLOCK",
  Numpad0: "kNUMPAD0",
  Numpad1: "kNUMPAD1",
  Numpad2: "kNUMPAD2",
  Numpad3: "kNUMPAD3",
  Numpad4: "kNUMPAD4",
  Numpad5: "kNUMPAD5",
  Numpad6: "kNUMPAD6",
  Numpad7: "kNUMPAD7",
  Numpad8: "kNUMPAD8",
  Numpad9: "kNUMPAD9",
  NumpadDivide: "kDIVIDE",
  NumpadMultiply: "kMULTIPLY",
  NumpadSubtract: "kSUBTRACT",
  NumpadAdd: "kADD",
  NumpadEnter: "kNUMPADENTER",
  NumpadDecimal: "kNUMPAD_DECIMAL",

  // Прочее
  PrintScreen: "kPRINTSCREEN",
  ScrollLock: "kSCROLL",
  Pause: "kPAUSE",
};
export const keybindsGroups = new Map<string, { [action: string]: [string, string] }>([
  [
    "direction",
    {
      left: [NO_KEY, NO_KEY],
      right: [NO_KEY, NO_KEY],
      up: [NO_KEY, NO_KEY],
      down: [NO_KEY, NO_KEY],
    }
  ],
  [
    "movement",
    {
      forward: [NO_KEY, NO_KEY],
      back: [NO_KEY, NO_KEY],
      lstrafe: [NO_KEY, NO_KEY],
      rstrafe: [NO_KEY, NO_KEY],
      jump: [NO_KEY, NO_KEY],
      crouch: [NO_KEY, NO_KEY],
      accel: [NO_KEY, NO_KEY],
      sprint_toggle: [NO_KEY, NO_KEY],
      llookout: [NO_KEY, NO_KEY],
      rlookout: [NO_KEY, NO_KEY],
    },
  ],
  [
    "weapons",
    {
      wpn_1: [NO_KEY, NO_KEY],
      wpn_2: [NO_KEY, NO_KEY],
      wpn_3: [NO_KEY, NO_KEY],
      wpn_4: [NO_KEY, NO_KEY],
      wpn_5: [NO_KEY, NO_KEY],
      wpn_6: [NO_KEY, NO_KEY],
      wpn_next: [NO_KEY, NO_KEY],
      next_slot: [NO_KEY, NO_KEY],
      prev_slot: [NO_KEY, NO_KEY],
      wpn_fire: [NO_KEY, NO_KEY],
      wpn_zoom: [NO_KEY, NO_KEY],
      wpn_zoom_second: [NO_KEY, NO_KEY],
      wpn_reload: [NO_KEY, NO_KEY],
      wpn_func: [NO_KEY, NO_KEY],
      wpn_firemode_next: [NO_KEY, NO_KEY],
      wpn_firemode_prev: [NO_KEY, NO_KEY],
      custom11: [NO_KEY, NO_KEY],
      wpn_quick_unload: [NO_KEY, NO_KEY],
      move_all_stack_items: [NO_KEY, NO_KEY],
    },
  ],
  [
    "inventory",
    {
      inventory: [NO_KEY, NO_KEY],
      active_jobs: [NO_KEY, NO_KEY],
      torch: [NO_KEY, NO_KEY],
      show_detector: [NO_KEY, NO_KEY],
      night_vision: [NO_KEY, NO_KEY],
      quick_use_1: [NO_KEY, NO_KEY],
      quick_use_2: [NO_KEY, NO_KEY],
      quick_use_3: [NO_KEY, NO_KEY],
      quick_use_4: [NO_KEY, NO_KEY],
      drop: [NO_KEY, NO_KEY],
    },
  ],
  [
    "common",
    {
      pause: [NO_KEY, NO_KEY],
      use: [NO_KEY, NO_KEY],
      talk_switch_to_trade: [NO_KEY, NO_KEY],
      screenshot: [NO_KEY, NO_KEY],
      quit: [NO_KEY, NO_KEY],
      console: [NO_KEY, NO_KEY],
      quick_save: [NO_KEY, NO_KEY],
      quick_load: [NO_KEY, NO_KEY],
      cam_1: [NO_KEY, NO_KEY],
      cam_2: [NO_KEY, NO_KEY],
      cam_3: [NO_KEY, NO_KEY],
      insert: [NO_KEY, NO_KEY],
      numpad0: [NO_KEY, NO_KEY],
      numpad1: [NO_KEY, NO_KEY],
      numpad2: [NO_KEY, NO_KEY],
      numpad3: [NO_KEY, NO_KEY],
      numpad4: [NO_KEY, NO_KEY],
      numpad5: [NO_KEY, NO_KEY],
      numpad6: [NO_KEY, NO_KEY],
      numpad7: [NO_KEY, NO_KEY],
      numpad8: [NO_KEY, NO_KEY],
      numpad9: [NO_KEY, NO_KEY],
      numpad_enter: [NO_KEY, NO_KEY],
      custom1: [NO_KEY, NO_KEY],
      custom2: [NO_KEY, NO_KEY],
      custom3: [NO_KEY, NO_KEY],
      custom4: [NO_KEY, NO_KEY],
      custom5: [NO_KEY, NO_KEY],
      custom6: [NO_KEY, NO_KEY],
      custom7: [NO_KEY, NO_KEY],
      custom8: [NO_KEY, NO_KEY],
      custom9: [NO_KEY, NO_KEY],
      custom10: [NO_KEY, NO_KEY],
    },
  ],
  [
    "multiplayer",
    {
      artefact: [NO_KEY, NO_KEY],
      scores: [NO_KEY, NO_KEY],
      chat: [NO_KEY, NO_KEY],
      chat_team: [NO_KEY, NO_KEY],
      buy_menu: [NO_KEY, NO_KEY],
      skin_menu: [NO_KEY, NO_KEY],
      team_menu: [NO_KEY, NO_KEY],
      vote_begin: [NO_KEY, NO_KEY],
      vote: [NO_KEY, NO_KEY],
      vote_yes: [NO_KEY, NO_KEY],
      vote_no: [NO_KEY, NO_KEY],
      speech_menu_0: [NO_KEY, NO_KEY],
      speech_menu_1: [NO_KEY, NO_KEY],
    },
  ],
  [
    "other",
    {
      pda_map_zoom_reset: [NO_KEY, NO_KEY],
      ui_accept: [NO_KEY, NO_KEY],
      ui_tab_next: [NO_KEY, NO_KEY],
      pda_map_show_actor: [NO_KEY, NO_KEY],
      pda_map_show_legend: [NO_KEY, NO_KEY],
      ui_button_6: [NO_KEY, NO_KEY],
      pda_map_zoom_in: [NO_KEY, NO_KEY],
      cam_zoom_in: [NO_KEY, NO_KEY],
      ui_tab_prev: [NO_KEY, NO_KEY],
      ui_back: [NO_KEY, NO_KEY],
      ui_move_left: [NO_KEY, NO_KEY],
      ui_button_8: [NO_KEY, NO_KEY],
      ui_button_2: [NO_KEY, NO_KEY],
      ui_button_4: [NO_KEY, NO_KEY],
      ui_move_right: [NO_KEY, NO_KEY],
      pda_map_zoom_out: [NO_KEY, NO_KEY],
      cam_zoom_out: [NO_KEY, NO_KEY],
      editor: [NO_KEY, NO_KEY],
      ui_button_3: [NO_KEY, NO_KEY],
      talk_log_scroll_up: [NO_KEY, NO_KEY],
      map: [NO_KEY, NO_KEY],
      ui_button_7: [NO_KEY, NO_KEY],
      ui_button_0: [NO_KEY, NO_KEY],
      pda_filter_toggle: [NO_KEY, NO_KEY],
      contacts: [NO_KEY, NO_KEY],
      ui_move_up: [NO_KEY, NO_KEY],
      talk_log_scroll_down: [NO_KEY, NO_KEY],
      ui_move_down: [NO_KEY, NO_KEY],
      ui_button_5: [NO_KEY, NO_KEY],
      ui_button_1: [NO_KEY, NO_KEY],
      enter: [NO_KEY, NO_KEY],
      ui_button_9: [NO_KEY, NO_KEY],
    },
  ],
]);
