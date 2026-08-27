-- ============================================================================
--  Индексатор метрических книг — схема базы данных (SQLite)
--  Версия схемы: 1
--
--  Принципы:
--   1. Форма ввода и хранение разделены. Порядок полей на экране можно менять
--      как угодно, эта схема при этом не трогается.
--   2. Единица хранения — не строка таблицы, а запись метрической книги
--      с набором упомянутых в ней персон.
--   3. Всё, что вводится руками, попадает в usage_stat и после этого
--      участвует в ранжировании подсказок по частоте.
--   4. Поиск по префиксу идёт по колонкам *_norm. Встроенный NOCASE в SQLite
--      понимает только латиницу, поэтому нормализованные значения (нижний
--      регистр, ё → е, схлопнутые пробелы) готовит приложение, а не база.
-- ============================================================================

-- journal_mode здесь намеренно не задаётся: WAL — настройка соединения, её
-- включает приложение при открытии базы. В схеме она ломает сборку на сетевых
-- и виртуальных файловых системах, где WAL не поддерживается.
PRAGMA foreign_keys = ON;

CREATE TABLE schema_version (
  version     INTEGER NOT NULL,
  applied_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- ============================================================================
--  СПРАВОЧНИКИ. Поставляются вместе с приложением, пополняются пользователем.
-- ============================================================================

-- Справочник русских имён. Перенесён с листа «Имена» исходного Индексатора
-- (автор А.К., на основе Словаря русских личных имён).
-- Используется для определения пола, построения отчеств и осовременивания.
CREATE TABLE name_dict (
  id          INTEGER PRIMARY KEY,
  gender      TEXT    CHECK (gender IN ('М','Ж')),
  name        TEXT    NOT NULL,
  name_norm   TEXT    NOT NULL,
  variant     TEXT,              -- разговорный или иной вариант написания
  base_name   TEXT,              -- имя-основа, к которому вариант приводится
  usage_note  TEXT,              -- пометы вида «Стар. редк.»
  declension  TEXT,              -- склонения и связи из словаря
  genitive    TEXT,              -- родительный падеж
  patr_old_m  TEXT,              -- отчество старой формы, мужское
  patr_old_f  TEXT,              -- отчество старой формы, женское
  patr_m      TEXT,              -- отчество современной формы, мужское
  patr_f      TEXT,              -- отчество современной формы, женское
  source      TEXT
);
CREATE INDEX ix_name_dict_norm   ON name_dict (name_norm);
CREATE INDEX ix_name_dict_gender ON name_dict (gender, name_norm);
CREATE INDEX ix_name_dict_base   ON name_dict (base_name);

-- Все написания имён одной таблицей: заголовочные имена, варианты написания
-- и все формы отчеств. Нужна для двух вещей: разбора строки ИОФ на части
-- и пословных подсказок при вводе.
--
-- Отдельная таблица, а не колонки *_norm в name_dict, потому что искать
-- приходится по любой из шести форм, и каждая должна быть проиндексирована.
CREATE TABLE name_form (
  id         INTEGER PRIMARY KEY,
  name_id    INTEGER NOT NULL REFERENCES name_dict (id) ON DELETE CASCADE,
  form       TEXT    NOT NULL,
  form_norm  TEXT    NOT NULL,
  kind       TEXT    NOT NULL CHECK (kind IN ('name','variant','patr_old_m','patr_old_f','patr_m','patr_f')),
  gender     TEXT,
  priority   INTEGER NOT NULL DEFAULT 0   -- 0 заголовочное написание, 1 вариант
);
CREATE INDEX ix_name_form_kind ON name_form (kind, form_norm);
CREATE INDEX ix_name_form_norm ON name_form (form_norm, priority);

-- Плоские перечни: архивы, церкви, губернии, уезды, звания, вероисповедания,
-- причины смерти, типы населённых пунктов, окончания фамилий и прочее.
-- kind — техническое имя перечня, см. таблицу lookup_kind.
CREATE TABLE lookup (
  id          INTEGER PRIMARY KEY,
  kind        TEXT    NOT NULL,
  value       TEXT    NOT NULL,
  value_norm  TEXT    NOT NULL,
  sort_order  INTEGER,
  origin      TEXT    NOT NULL DEFAULT 'seed' CHECK (origin IN ('seed','user')),
  created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
  UNIQUE (kind, value)
);
CREATE INDEX ix_lookup_kind ON lookup (kind, value_norm);

-- Человекочитаемые названия перечней — для экрана редактирования справочников.
CREATE TABLE lookup_kind (
  kind        TEXT PRIMARY KEY,
  title       TEXT NOT NULL,
  editable    INTEGER NOT NULL DEFAULT 1,   -- 0 = системный, менять нельзя
  autoextend  INTEGER NOT NULL DEFAULT 1    -- 1 = пополнять автоматически при вводе
);

-- Роли персон в записи. Определяют состав полей формы для каждого раздела.
-- section: 1 рождения, 2 браки, 3 смерти, 0 — общая для всех разделов.
CREATE TABLE role (
  code        TEXT PRIMARY KEY,
  title       TEXT    NOT NULL,
  section     INTEGER NOT NULL,
  sort_order  INTEGER NOT NULL,
  gender      TEXT,                          -- жёстко заданный пол роли, если есть
  age_min     INTEGER,                       -- коридор возраста для прогноза
  age_max     INTEGER                        -- года рождения (ответы: 17-55, 6-60, 17-60)
);

-- Справочник населённых пунктов, он же источник данных для выгрузки в Familio.
-- На листе «НП» присланного файла данных нет — заполняется по мере работы.
CREATE TABLE place (
  id              INTEGER PRIMARY KEY,
  name            TEXT NOT NULL,
  name_norm       TEXT NOT NULL,
  np_type         TEXT,           -- д., с., г., погост и т. д.
  guberniya       TEXT,
  uyezd           TEXT,
  volost          TEXT,
  short_location  TEXT,           -- краткая сборка для таблиц
  full_location   TEXT,           -- полная сборка для Familio
  familio_url     TEXT,
  origin          TEXT NOT NULL DEFAULT 'user',
  created_at      TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE (name, np_type, uyezd, guberniya)
);
CREATE INDEX ix_place_norm ON place (name_norm);

-- ============================================================================
--  ДАННЫЕ ПОЛЬЗОВАТЕЛЯ
-- ============================================================================

-- Дело: архив, фонд, опись, дело, приход, год. Вводится один раз и держится
-- неизменным, пока пользователь сам его не поменяет (требование 3.4).
CREATE TABLE mk_case (
  id            INTEGER PRIMARY KEY,
  archive       TEXT,
  fond          TEXT,
  opis          TEXT,
  delo          TEXT,
  church        TEXT,
  village       TEXT,
  uyezd         TEXT,
  guberniya     TEXT,
  year          INTEGER,
  parish_key    TEXT,          -- church|village|uyezd|guberniya, связывает годы одного прихода
  indexer       TEXT,          -- кто проводит индексацию
  note          TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at    TEXT
);
CREATE INDEX ix_case_parish ON mk_case (parish_key);

-- Причт дела: до трёх церковнослужителей, подставляются в каждую запись.
CREATE TABLE clergy (
  id        INTEGER PRIMARY KEY,
  case_id   INTEGER NOT NULL REFERENCES mk_case (id) ON DELETE CASCADE,
  slot      INTEGER NOT NULL CHECK (slot BETWEEN 1 AND 3),
  fio       TEXT,
  rank      TEXT,
  note      TEXT,
  UNIQUE (case_id, slot)
);

-- Запись метрической книги. Событийные атрибуты — здесь; всё, что описывает
-- человека, живёт в person_mention.
CREATE TABLE entry (
  id             INTEGER PRIMARY KEY,
  case_id        INTEGER NOT NULL REFERENCES mk_case (id) ON DELETE CASCADE,
  section        INTEGER NOT NULL CHECK (section IN (1,2,3)),
  page           TEXT,
  no_male        INTEGER,     -- счёт родившихся или умерших мужского пола
  no_female      INTEGER,     -- то же женского; для браков используется no_male
  event_day      INTEGER,     -- рождение / бракосочетание / смерть
  event_month    INTEGER,
  event_year     INTEGER,
  rite_day       INTEGER,     -- крещение / погребение; для браков пусто
  rite_month     INTEGER,
  rite_year      INTEGER,
  note           TEXT,
  uncertain      TEXT,        -- перечень полей, прочитанных неуверенно (JSON-массив)
  created_by     TEXT,        -- кто набрал (передача дела между людьми, ответ 56)
  created_at     TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at     TEXT
);
CREATE INDEX ix_entry_case ON entry (case_id, section, id);

-- Сводная персона: один человек, упомянутый в нескольких записях.
-- Наполняется при связывании упоминаний (требование П-1).
CREATE TABLE person (
  id               INTEGER PRIMARY KEY,
  surname          TEXT,
  first_name       TEXT,
  patronymic       TEXT,
  fio_norm         TEXT NOT NULL,
  gender           TEXT,
  rank             TEXT,
  place_id         INTEGER REFERENCES place (id),
  birth_year_from  INTEGER,
  birth_year_to    INTEGER,
  mentions_count   INTEGER NOT NULL DEFAULT 0,
  created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX ix_person_fio   ON person (fio_norm);
CREATE INDEX ix_person_place ON person (place_id);

-- Родственные связи между персонами. Именно отсюда берётся подстановка
-- матери при вводе отца (требование А-4).
CREATE TABLE person_link (
  person_id   INTEGER NOT NULL REFERENCES person (id) ON DELETE CASCADE,
  related_id  INTEGER NOT NULL REFERENCES person (id) ON DELETE CASCADE,
  relation    TEXT    NOT NULL,     -- супруг, супруга, отец, мать, сын, дочь
  weight      INTEGER NOT NULL DEFAULT 1,   -- сколько раз связь подтверждалась
  PRIMARY KEY (person_id, related_id, relation)
);
CREATE INDEX ix_link_related ON person_link (related_id);

-- Упоминание персоны в конкретной записи.
-- Оригинальное и современное написание хранятся рядом: при включённом
-- осовременивании в результате должны быть оба варианта (требование П-3).
CREATE TABLE person_mention (
  id                INTEGER PRIMARY KEY,
  entry_id          INTEGER NOT NULL REFERENCES entry (id) ON DELETE CASCADE,
  role_code         TEXT    NOT NULL REFERENCES role (code),
  person_id         INTEGER REFERENCES person (id),
  sort_order        INTEGER NOT NULL DEFAULT 0,

  surname           TEXT,          -- как записано в книге
  first_name        TEXT,
  patronymic        TEXT,
  surname_modern    TEXT,          -- современная форма
  first_name_modern TEXT,
  patronymic_modern TEXT,
  maiden_surname    TEXT,          -- девичья фамилия

  gender            TEXT,
  rank              TEXT,          -- звание
  confession        TEXT,          -- вероисповедание
  place_id          INTEGER REFERENCES place (id),

  age_years         INTEGER,       -- возраст умершего
  age_months        INTEGER,
  age_days          INTEGER,
  death_cause       TEXT,
  marriage_order    TEXT,          -- каким браком

  birth_year_from   INTEGER,       -- прогноз по возрастному коридору роли
  birth_year_to     INTEGER,

  note              TEXT,
  uncertain         TEXT
);
CREATE INDEX ix_mention_entry  ON person_mention (entry_id, sort_order);
CREATE INDEX ix_mention_person ON person_mention (person_id);

-- ============================================================================
--  ЧАСТОТЫ ДЛЯ АВТОПОДСТАНОВКИ
--  Главная претензия заказчика: подсказки идут по алфавиту, а должны идти
--  по частоте. Порядок выдачи: текущее дело → приход → вся база → словарь.
-- ============================================================================

CREATE TABLE usage_stat (
  kind          TEXT    NOT NULL,   -- совпадает с lookup.kind либо first_name / surname / place / person
  scope         TEXT    NOT NULL CHECK (scope IN ('case','parish','global')),
  scope_key     TEXT    NOT NULL,   -- id дела, ключ прихода или пустая строка
  value         TEXT    NOT NULL,
  value_norm    TEXT    NOT NULL,
  count         INTEGER NOT NULL DEFAULT 0,
  last_used_at  TEXT,
  PRIMARY KEY (kind, scope, scope_key, value)
);
CREATE INDEX ix_usage_prefix ON usage_stat (kind, scope, scope_key, value_norm);

-- ============================================================================
--  ПАМЯТЬ О ПЕРСОНАХ
--
--  Заказчик 17.08.2026: «я хочу чтобы во всех полях ИОФ индексатор
--  предугадывал уже занесённого в базу человека, а не отдельно имя, отчество
--  и фамилию... как только ты выбираешь существующую персону, то и НП, и его
--  звание, и все данные его жены тут же должны быть заполнены автоматически».
--
--  Отсюда две таблицы. Первая помнит персон целиком, вместе с населённым
--  пунктом и званием: выбор строки заполняет три поля разом. Вторая помнит,
--  кто чья жена: выбор отца заполняет мать.
--
--  Это отдельные таблицы, а не колонки в person_mention, потому что механизм
--  обновления базы у пользователя умеет создавать недостающие таблицы,
--  но не умеет добавлять колонки в существующие.
-- ============================================================================

CREATE TABLE person_index (
  id            INTEGER PRIMARY KEY,
  iof           TEXT    NOT NULL,      -- как записано в книге
  iof_norm      TEXT    NOT NULL,      -- ключ поиска по префиксу
  place         TEXT,
  rank          TEXT,
  gender        TEXT,
  uses          INTEGER NOT NULL DEFAULT 0,
  last_used_at  TEXT,
  UNIQUE (iof, place, rank)
);
CREATE INDEX ix_person_index_norm ON person_index (iof_norm, uses DESC);

CREATE TABLE spouse_index (
  id             INTEGER PRIMARY KEY,
  husband_norm   TEXT    NOT NULL,     -- ключ поиска: ИОФ мужа
  wife_iof       TEXT    NOT NULL,
  wife_place     TEXT,
  wife_rank      TEXT,
  uses           INTEGER NOT NULL DEFAULT 0,
  last_used_at   TEXT,
  UNIQUE (husband_norm, wife_iof)
);
CREATE INDEX ix_spouse_husband ON spouse_index (husband_norm, uses DESC);

-- ============================================================================
--  НАСТРОЙКИ
-- ============================================================================

CREATE TABLE setting (
  key    TEXT PRIMARY KEY,
  value  TEXT
);
