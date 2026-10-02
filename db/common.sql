-- Общий файл программы — `genmetric-общее.sqlite` рядом с приходами
-- (спека 2026-10-02, п. 1.3 и 2). Каждый приход — свой файл SQLite; здесь то,
-- что у приходов общее: их перечень, настройки окна и справочники, которые
-- человек пополняет сам (Роман 28.09.2026: «справочники НП и званий — общие»).
--
-- Файл выполняется при каждом открытии общего файла: только CREATE … IF NOT
-- EXISTS. Тот же файл выполняет db/test_parish.py.

-- Перечень приходов. file — путь относительно папки данных; name пусто у
-- первого прихода (прежняя genmetric.sqlite): он называется по селу из дела.
-- source_* — из какого файла Excel приход импортирован (повторный импорт
-- того же файла программа замечает и спрашивает, что делать).
CREATE TABLE IF NOT EXISTS parish (
  id          INTEGER PRIMARY KEY,
  name        TEXT,
  file        TEXT NOT NULL UNIQUE,
  created_at  TEXT NOT NULL DEFAULT (datetime('now')),
  opened_at   TEXT,
  source_name TEXT,
  source_size INTEGER
);

-- Настройки окна (масштаб, свёрнут ли причт, карточка «Для Familio») и
-- current_parish — какой приход открыт.
CREATE TABLE IF NOT EXISTS setting (
  key    TEXT PRIMARY KEY,
  value  TEXT
);

-- Населённые пункты: одна строка на название. В приходе строк с одним
-- названием бывает несколько («двойник» без подробностей рядом с пунктом
-- поставки) — сюда идёт самая полная.
CREATE TABLE IF NOT EXISTS place (
  name            TEXT NOT NULL,
  name_norm       TEXT NOT NULL UNIQUE,
  np_type         TEXT,
  guberniya       TEXT,
  uyezd           TEXT,
  volost          TEXT,
  short_location  TEXT,
  full_location   TEXT,
  familio_url     TEXT,
  origin          TEXT,
  created_at      TEXT,
  updated_at      TEXT
);

CREATE TABLE IF NOT EXISTS place_renamed (
  old_norm    TEXT PRIMARY KEY,
  renamed_at  TEXT
);

-- Пополненное человеком в перечнях (lookup, origin = 'user'): звания,
-- причины смерти, родство, архивы, церкви, уезды, губернии, типы НП.
CREATE TABLE IF NOT EXISTS lookup (
  kind        TEXT NOT NULL,
  value       TEXT NOT NULL,
  value_norm  TEXT NOT NULL,
  created_at  TEXT,
  UNIQUE (kind, value_norm)
);

-- Решения окна сверки имён.
CREATE TABLE IF NOT EXISTS name_alias (
  kind        TEXT NOT NULL,
  form        TEXT NOT NULL,
  form_norm   TEXT NOT NULL,
  target      TEXT,
  gender      TEXT,
  created_at  TEXT,
  UNIQUE (kind, form_norm)
);
