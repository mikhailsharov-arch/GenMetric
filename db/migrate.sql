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

-- Исправление прошлой поставки. Два звания были перенесены из Excel не в тот
-- перечень: «крестьянский сын» попал к женским, «крестьянская вдова после
-- 1-го брака» — к мужским. Из поставки они убраны, но у тех, кто уже поставил
-- прежнюю сборку, остались бы навсегда: правило «только дополняем» ничего
-- не удаляет. Поэтому — точечное удаление, и только для значений из поставки:
-- заведённое человеком не трогаем никогда.
DELETE FROM lookup
 WHERE origin = 'seed'
   AND ((kind = 'rank_f' AND value = 'крестьянский сын')
     OR (kind = 'rank_m' AND value = 'крестьянская вдова после 1-го брака'));

-- Настройки пользователя не перезаписываем: добавляем только новые ключи.
INSERT OR IGNORE INTO setting (key, value) SELECT key, value FROM seed.setting;

-- --- починка данных: номер девочек в женскую колонку ------------------------
-- До сборки 13.09.2026 форма писала счёт в мужскую колонку независимо от пола
-- ребёнка (инцидент 20260913). Роман 21.09.2026: «Да, конечно же нужно
-- исправить, так как я планирую продолжать индексацию уже в этой программе».
--
-- Условие точное и повторяемое: номер стоит только в мужской колонке, а ребёнок
-- записан девочкой. Записи после починки у девочек имеют номер в женской
-- колонке и под условие не попадают; мальчики — тоже; ребёнок без пола
-- не трогается: угадывать нельзя. Повторный прогон ничего не находит.
-- Число исправленных складывается в setting и показывается на экране
-- «О программе» — человек должен увидеть, что с его данными что-то сделали.
INSERT OR IGNORE INTO setting (key, value) VALUES ('repair_count_column', '0');

UPDATE setting
   SET value = CAST(CAST(value AS INTEGER) + (
        SELECT count(*) FROM entry e
         WHERE e.section = 1 AND e.no_male IS NOT NULL AND e.no_female IS NULL
           AND EXISTS (SELECT 1 FROM person_mention m
                        WHERE m.entry_id = e.id AND m.role_code = 'child' AND m.gender = 'Ж')
       ) AS TEXT)
 WHERE key = 'repair_count_column';

UPDATE entry
   SET no_female = no_male, no_male = NULL, updated_at = datetime('now')
 WHERE section = 1 AND no_male IS NOT NULL AND no_female IS NULL
   AND EXISTS (SELECT 1 FROM person_mention m
                WHERE m.entry_id = entry.id AND m.role_code = 'child' AND m.gender = 'Ж');

-- Ребёнок без пола (имя вне словаря до 13.09) с номером в мужской колонке —
-- не чиним, но считаем: «исправлено N» без этого читалось бы как «всё
-- исправлено» (ревьюер 21.09.2026). Число пересчитывается при каждом
-- обновлении, показывается на «О программе», Роман правит такие сам.
INSERT OR REPLACE INTO setting (key, value)
SELECT 'repair_unknown_sex', CAST(count(*) AS TEXT) FROM entry e
 WHERE e.section = 1 AND e.no_male IS NOT NULL AND e.no_female IS NULL
   AND EXISTS (SELECT 1 FROM person_mention m
                WHERE m.entry_id = e.id AND m.role_code = 'child' AND m.gender IS NULL);

-- Отпечаток поставки обновляем принудительно: по нему определяется, нужно ли
-- обновление в следующий раз. Ещё принудительно — счётчик починки выше.
UPDATE setting
   SET value = (SELECT value FROM seed.setting WHERE key = 'seed_stamp')
 WHERE key = 'seed_stamp';

COMMIT;
