-- ============================================================================
--  Обновление справочников в базе пользователя из поставки.
--
--  Зачем это существует. База копируется в папку пользователя только при
--  первой установке. Без этого файла всё, что мы правим в схеме и справочниках,
--  до человека просто не доезжает: он ставит новую версию поверх старой,
--  а работает по-прежнему со старой базой. Именно так вышло 13.08.2026 —
--  у тестировщика не появилась таблица name_form, и половина сборки молча
--  не работала.
--
--  Файл выполняется приложением при запуске, когда отпечаток поставки
--  (setting.seed_stamp) отличается от записанного в базе пользователя.
--  К этому моменту база поставки уже подключена под именем seed, а недостающие
--  таблицы созданы по её образцу.
--
--  Тот же файл прогоняется тестом db/test_upgrade.py — поэтому логика
--  обновления проверяется по-настоящему, хотя вызывающий её код на Rust
--  в песочнице не собирается.
--
--  ПРАВИЛО РАЗДЕЛЕНИЯ. Таблицы, которые пользователь не правит руками
--  (имена и их формы), заменяются целиком. Таблицы, которые он пополняет
--  сам (перечни, населённые пункты, настройки), только дополняются —
--  ничего введённого человеком не удаляется и не перезаписывается.
-- ============================================================================

-- Всё обновление одной транзакцией: либо проходит целиком, либо база остаётся
-- прежней. Транзакция живёт внутри этого файла, а не в вызывающем коде, потому
-- что Python и Rust по-разному обращаются с внешними транзакциями — а файл
-- должен вести себя одинаково и в приложении, и в тесте.
BEGIN;

-- --- то, что пользователь не редактирует: заменяем целиком -------------------
-- Порядок важен: name_form ссылается на name_dict.
DELETE FROM name_form;
DELETE FROM name_dict;

INSERT INTO name_dict
    (id, gender, name, name_norm, variant, base_name, usage_note, declension,
     genitive, patr_old_m, patr_old_f, patr_m, patr_f, source)
SELECT id, gender, name, name_norm, variant, base_name, usage_note, declension,
       genitive, patr_old_m, patr_old_f, patr_m, patr_f, source
  FROM seed.name_dict;

INSERT INTO name_form (id, name_id, form, form_norm, kind, gender, priority)
SELECT id, name_id, form, form_norm, kind, gender, priority
  FROM seed.name_form;

-- --- служебные перечни: обновляем названия, ничего не удаляем ---------------
INSERT OR REPLACE INTO lookup_kind (kind, title, editable, autoextend)
SELECT kind, title, editable, autoextend FROM seed.lookup_kind;

INSERT OR REPLACE INTO role (code, title, section, sort_order, gender, age_min, age_max)
SELECT code, title, section, sort_order, gender, age_min, age_max FROM seed.role;

-- --- то, что пользователь пополняет сам: только дополняем -------------------
-- UNIQUE(kind, value) не даст задвоить, а введённое человеком останется на месте.
INSERT OR IGNORE INTO lookup (kind, value, value_norm, sort_order, origin)
SELECT kind, value, value_norm, sort_order, origin FROM seed.lookup;

INSERT OR IGNORE INTO place
    (name, name_norm, np_type, guberniya, uyezd, volost,
     short_location, full_location, familio_url, origin)
SELECT name, name_norm, np_type, guberniya, uyezd, volost,
       short_location, full_location, familio_url, origin
  FROM seed.place;

-- Настройки пользователя не перезаписываем: добавляем только новые ключи.
INSERT OR IGNORE INTO setting (key, value) SELECT key, value FROM seed.setting;

-- Отпечаток поставки — единственная настройка, которую обновляем принудительно:
-- по ней определяется, нужно ли обновление в следующий раз.
UPDATE setting
   SET value = (SELECT value FROM seed.setting WHERE key = 'seed_stamp')
 WHERE key = 'seed_stamp';

COMMIT;
