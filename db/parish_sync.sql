-- Сверка справочников прихода с общим файлом (спека 2026-10-02, п. 2).
--
-- Выполняется на соединении прихода, к которому общий файл подключён как
-- `common` (ATTACH): при открытии прихода и после каждого сохранения, которое
-- пополняет справочники (карточка пункта, запись, дело, окно сверки).
-- В обе стороны: чего нет в общем файле — туда; чего нет в приходе — в него.
-- Повторный прогон ничего не меняет. Тот же файл выполняет db/test_parish.py.
--
-- Общее: населённые пункты с карточками и памятью переименований, пополненное
-- в перечнях, соответствия имён. НЕ общее: персоны, жёны, причт, частоты
-- подсказок, записи — их здесь нет и быть не должно.

-- ---------------------------------------------------------------------------
-- 1. Память переименований — объединение. Старое название переименованного
--    пункта нигде не заводится заново.
-- ---------------------------------------------------------------------------
--    Название, которое человек вернул (переименовал и передумал) или завёл
--    заново карточкой позже переименования, — снова обычный пункт: из памяти
--    переименований оно уходит и здесь, и в общем файле, и — по карточке в
--    общем файле — в остальных приходах. Иначе пункт навсегда пропал бы из
--    общих справочников (ревьюер 02.10.2026).
DELETE FROM common.place_renamed
 WHERE old_norm IN (SELECT r.old_norm FROM main.place_renamed r JOIN main.place p ON p.name_norm = r.old_norm
                     WHERE p.updated_at IS NOT NULL AND p.updated_at >= r.renamed_at);
DELETE FROM main.place_renamed
 WHERE EXISTS (SELECT 1 FROM main.place p WHERE p.name_norm = main.place_renamed.old_norm
                  AND p.updated_at IS NOT NULL AND p.updated_at >= main.place_renamed.renamed_at)
    OR EXISTS (SELECT 1 FROM common.place c WHERE c.name_norm = main.place_renamed.old_norm
                  AND c.updated_at IS NOT NULL AND c.updated_at >= main.place_renamed.renamed_at);
INSERT OR IGNORE INTO common.place_renamed (old_norm, renamed_at)
SELECT old_norm, renamed_at FROM main.place_renamed;
INSERT OR IGNORE INTO main.place_renamed (old_norm, renamed_at)
SELECT old_norm, coalesce(renamed_at, datetime('now')) FROM common.place_renamed;
DELETE FROM common.place WHERE name_norm IN (SELECT old_norm FROM common.place_renamed);

-- ---------------------------------------------------------------------------
-- 2. Населённые пункты. Пункт — это название (name_norm): форма хранит
--    название, запись ссылается на самую полную строку (place_find). Она же
--    сверяется с общим файлом.
--
--    Спор о подробностях одного пункта: заполненное побеждает пустое; из двух
--    разных заполненных — более позднее (updated_at; пусто — «не правили»,
--    старше любой правки). Время — с миллисекундами (place_save,
--    place_update): карточку заводят и тут же правят, и с точностью до секунды
--    правка считалась бы «не позже» и откатывалась (ревьюер 02.10.2026).
--    Название не затирается.
-- ---------------------------------------------------------------------------
DROP TABLE IF EXISTS temp.s_best;
CREATE TEMP TABLE s_best AS
SELECT p.id, p.name, p.name_norm, p.np_type, p.guberniya, p.uyezd, p.volost,
       p.short_location, p.full_location, p.familio_url, p.origin, p.created_at, p.updated_at,
       (trim(coalesce(p.np_type, '')) || trim(coalesce(p.guberniya, '')) || trim(coalesce(p.uyezd, ''))
        || trim(coalesce(p.volost, '')) || trim(coalesce(p.familio_url, ''))) <> '' AS filled,
       trim(coalesce(p.np_type, '')) || '|' || trim(coalesce(p.guberniya, '')) || '|'
        || trim(coalesce(p.uyezd, '')) || '|' || trim(coalesce(p.volost, '')) || '|'
        || trim(coalesce(p.familio_url, '')) AS sig
  FROM main.place p
 WHERE p.id = (SELECT b.id FROM main.place b WHERE b.name_norm = p.name_norm
                ORDER BY (b.full_location IS NULL OR trim(b.full_location) = ''), (b.origin = 'archive'), b.id
                LIMIT 1);

-- 2а. Из прихода в общий файл: новые пункты…
INSERT OR IGNORE INTO common.place (name, name_norm, np_type, guberniya, uyezd, volost,
                                    short_location, full_location, familio_url, origin, created_at, updated_at)
SELECT b.name, b.name_norm, b.np_type, b.guberniya, b.uyezd, b.volost,
       b.short_location, b.full_location, b.familio_url, b.origin, b.created_at, b.updated_at
  FROM temp.s_best b
 WHERE b.name_norm NOT IN (SELECT old_norm FROM common.place_renamed);

-- …и подробности: в пустую карточку или более поздняя правка.
UPDATE common.place
   SET (np_type, guberniya, uyezd, volost, short_location, full_location, familio_url, updated_at) =
       (SELECT b.np_type, b.guberniya, b.uyezd, b.volost, b.short_location, b.full_location,
               b.familio_url, b.updated_at
          FROM temp.s_best b WHERE b.name_norm = common.place.name_norm)
 WHERE EXISTS (
   SELECT 1 FROM temp.s_best b
    WHERE b.name_norm = common.place.name_norm AND b.filled
      AND b.sig <> trim(coalesce(common.place.np_type, '')) || '|' || trim(coalesce(common.place.guberniya, '')) || '|'
                   || trim(coalesce(common.place.uyezd, '')) || '|' || trim(coalesce(common.place.volost, '')) || '|'
                   || trim(coalesce(common.place.familio_url, ''))
      AND ((trim(coalesce(common.place.np_type, '')) || trim(coalesce(common.place.guberniya, ''))
            || trim(coalesce(common.place.uyezd, '')) || trim(coalesce(common.place.volost, ''))
            || trim(coalesce(common.place.familio_url, ''))) = ''
           OR coalesce(b.updated_at, '') > coalesce(common.place.updated_at, '')));

-- 2б. Из общего файла в приход: новые пункты…
INSERT OR IGNORE INTO main.place (name, name_norm, np_type, guberniya, uyezd, volost,
                                  short_location, full_location, familio_url, origin, updated_at)
SELECT c.name, c.name_norm, c.np_type, c.guberniya, c.uyezd, c.volost,
       c.short_location, c.full_location, c.familio_url, coalesce(c.origin, 'user'), c.updated_at
  FROM common.place c
 WHERE NOT EXISTS (SELECT 1 FROM main.place p WHERE p.name_norm = c.name_norm)
   AND c.name_norm NOT IN (SELECT old_norm FROM main.place_renamed);

-- …и подробности. После шага 2а расхождение значит одно из двух: карточка
-- прихода пуста или общая — более поздняя; в обоих случаях верна общая.
-- OR IGNORE: если в приходе уже есть другая строка с точно такими
-- подробностями (UNIQUE), оставляем как есть.
UPDATE OR IGNORE main.place
   SET (np_type, guberniya, uyezd, volost, short_location, full_location, familio_url, updated_at) =
       (SELECT c.np_type, c.guberniya, c.uyezd, c.volost, c.short_location, c.full_location,
               c.familio_url, c.updated_at
          FROM common.place c WHERE c.name_norm = main.place.name_norm)
 WHERE id IN (
   SELECT b.id FROM temp.s_best b JOIN common.place c ON c.name_norm = b.name_norm
    WHERE (trim(coalesce(c.np_type, '')) || trim(coalesce(c.guberniya, '')) || trim(coalesce(c.uyezd, ''))
           || trim(coalesce(c.volost, '')) || trim(coalesce(c.familio_url, ''))) <> ''
      AND b.sig <> trim(coalesce(c.np_type, '')) || '|' || trim(coalesce(c.guberniya, '')) || '|'
                   || trim(coalesce(c.uyezd, '')) || '|' || trim(coalesce(c.volost, '')) || '|'
                   || trim(coalesce(c.familio_url, '')));

DROP TABLE IF EXISTS temp.s_best;

-- ---------------------------------------------------------------------------
-- 3. Перечни: пополненное человеком. Сравнение по ключу поиска (value_norm),
--    чтобы «Мещанин» и «мещанин» не стали двумя значениями.
-- ---------------------------------------------------------------------------
INSERT OR IGNORE INTO common.lookup (kind, value, value_norm, created_at)
SELECT kind, value, value_norm, created_at FROM main.lookup WHERE origin = 'user';

INSERT OR IGNORE INTO main.lookup (kind, value, value_norm, sort_order, origin, created_at)
SELECT c.kind, c.value, c.value_norm,
       (SELECT coalesce(max(l.sort_order), 0) FROM main.lookup l WHERE l.kind = c.kind)
         + 10 * row_number() OVER (PARTITION BY c.kind ORDER BY c.rowid),
       'user', coalesce(c.created_at, datetime('now'))
  FROM common.lookup c
 WHERE EXISTS (SELECT 1 FROM main.lookup_kind k WHERE k.kind = c.kind)
   AND NOT EXISTS (SELECT 1 FROM main.lookup l WHERE l.kind = c.kind AND l.value_norm = c.value_norm);

-- ---------------------------------------------------------------------------
-- 4. Соответствия имён из окна сверки. Повторное решение заменяет прежнее
--    (alias_save), поэтому из двух разных верно более позднее.
-- ---------------------------------------------------------------------------
INSERT INTO common.name_alias (kind, form, form_norm, target, gender, created_at)
SELECT kind, form, form_norm, target, gender, created_at FROM main.name_alias WHERE true
    ON CONFLICT (kind, form_norm) DO UPDATE
   SET form = excluded.form, target = excluded.target, gender = excluded.gender,
       created_at = excluded.created_at
 WHERE coalesce(excluded.created_at, '') > coalesce(name_alias.created_at, '');

INSERT INTO main.name_alias (kind, form, form_norm, target, gender, created_at)
SELECT kind, form, form_norm, target, gender, coalesce(created_at, datetime('now'))
  FROM common.name_alias WHERE true
    ON CONFLICT (kind, form_norm) DO UPDATE
   SET form = excluded.form, target = excluded.target, gender = excluded.gender,
       created_at = excluded.created_at
 WHERE coalesce(excluded.target, '') <> coalesce(name_alias.target, '')
    OR coalesce(excluded.gender, '') <> coalesce(name_alias.gender, '');
