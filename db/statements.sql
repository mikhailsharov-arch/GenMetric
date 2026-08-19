-- ============================================================================
--  Запросы записи и чтения, общие для приложения и тестов.
--
--  Зачем файл. Код на Rust в песочнице не собирается, поэтому запросы, которые
--  трогают данные пользователя, обязаны проверяться отдельно. Приложение
--  и тест db/test_entry.py читают отсюда один и тот же текст — значит
--  проверяется именно то, что работает у человека, а не похожая копия.
--
--  Формат: блоки разделены строкой «-- @имя». Загрузчики есть в обоих языках.
-- ============================================================================

-- @case_upsert
-- Шапка дела. Заводится один раз и держится, пока пользователь сам не изменит:
-- архив, фонд, опись, дело, приход, год. parish_key связывает годы одного
-- прихода — по нему переносится накопленная статистика подсказок.
INSERT INTO mk_case (id, archive, fond, opis, delo, church, village, uyezd,
                     guberniya, year, parish_key, indexer, updated_at)
VALUES (:id, :archive, :fond, :opis, :delo, :church, :village, :uyezd,
        :guberniya, :year, :parish_key, :indexer, datetime('now'))
ON CONFLICT(id) DO UPDATE SET
    archive = excluded.archive, fond = excluded.fond, opis = excluded.opis,
    delo = excluded.delo, church = excluded.church, village = excluded.village,
    uyezd = excluded.uyezd, guberniya = excluded.guberniya, year = excluded.year,
    parish_key = excluded.parish_key, indexer = excluded.indexer,
    updated_at = datetime('now');

-- @entry_insert
INSERT INTO entry (case_id, section, page, no_male, no_female,
                   event_day, event_month, event_year,
                   rite_day, rite_month, rite_year, note, uncertain, created_by)
VALUES (:case_id, :section, :page, :no_male, :no_female,
        :event_day, :event_month, :event_year,
        :rite_day, :rite_month, :rite_year, :note, :uncertain, :created_by);

-- @entry_update
UPDATE entry SET page = :page, no_male = :no_male, no_female = :no_female,
                 event_day = :event_day, event_month = :event_month, event_year = :event_year,
                 rite_day = :rite_day, rite_month = :rite_month, rite_year = :rite_year,
                 note = :note, uncertain = :uncertain, updated_at = datetime('now')
 WHERE id = :id;

-- @mentions_clear
-- Персоны записи переписываются целиком: так проще и надёжнее, чем сверять
-- построчно, а записей на одну правку немного.
DELETE FROM person_mention WHERE entry_id = :entry_id;

-- @mention_insert
INSERT INTO person_mention
    (entry_id, role_code, sort_order, surname, first_name, patronymic,
     surname_modern, first_name_modern, patronymic_modern, maiden_surname,
     gender, rank, confession, place_id, note, uncertain,
     birth_year_from, birth_year_to)
VALUES (:entry_id, :role_code, :sort_order, :surname, :first_name, :patronymic,
        :surname_modern, :first_name_modern, :patronymic_modern, :maiden_surname,
        :gender, :rank, :confession, :place_id, :note, :uncertain,
        :birth_year_from, :birth_year_to);

-- @place_find
SELECT id FROM place WHERE name_norm = :name_norm LIMIT 1;

-- @place_insert
-- Населённый пункт заводится по первому упоминанию. Подробности (губерния,
-- уезд, волость, ссылка на Familio) человек заполняет позже, перед выгрузкой:
-- в рабочем файле по Борисоглебскому у всех 159 пунктов ссылка проставлена,
-- значит это обязательный шаг, но не в момент набора записи.
INSERT INTO place (name, name_norm, origin) VALUES (:name, :name_norm, 'user');

-- @lookup_extend
-- Автопополнение справочников: значение, которого нет в перечне, добавляется
-- при сохранении записи. Пункт 3 отчёта Романа о тестировании.
-- Пополняются только перечни, помеченные autoextend в lookup_kind.
INSERT OR IGNORE INTO lookup (kind, value, value_norm, sort_order, origin)
SELECT :kind, :value, :value_norm,
       (SELECT coalesce(max(sort_order), 0) + 10 FROM lookup WHERE kind = :kind),
       'user'
 WHERE EXISTS (SELECT 1 FROM lookup_kind WHERE kind = :kind AND autoextend = 1);

-- @usage_bump
-- Частота использования: на ней держится порядок подсказок.
-- Считается в трёх охватах сразу — дело, приход и вся база, — потому что
-- выдача идёт именно в таком порядке (требование А-1).
INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)
VALUES (:kind, :scope, :scope_key, :value, :value_norm, 1, datetime('now'))
ON CONFLICT(kind, scope, scope_key, value) DO UPDATE SET
    count = count + 1, last_used_at = datetime('now');

-- @entry_list
-- Список набранных записей дела: номер, дата, имя ребёнка — чтобы вернуться
-- и поправить.
SELECT e.id, e.page, e.no_male, e.no_female,
       e.event_day, e.event_month, e.event_year,
       (SELECT trim(coalesce(m.first_name, '') || ' ' || coalesce(m.patronymic, '')
                    || ' ' || coalesce(m.surname, ''))
          FROM person_mention m
         WHERE m.entry_id = e.id AND m.role_code = 'child') AS child
  FROM entry e
 WHERE e.case_id = :case_id AND e.section = :section
 ORDER BY e.id DESC;

-- @mentions_of_entry
SELECT role_code, sort_order, surname, first_name, patronymic,
       surname_modern, first_name_modern, patronymic_modern, maiden_surname,
       gender, rank, confession, place_id, note, uncertain
  FROM person_mention
 WHERE entry_id = :entry_id
 ORDER BY sort_order;

-- @person_remember
-- Запоминает персону целиком: ИОФ вместе с населённым пунктом и званием.
-- Ради этого всё и делается — выбор строки заполняет три поля разом.
INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses, last_used_at)
VALUES (:iof, :iof_norm, :place, :rank, :gender, 1, datetime('now'))
ON CONFLICT(iof, place, rank) DO UPDATE SET
    uses = uses + 1, last_used_at = datetime('now'),
    gender = coalesce(excluded.gender, gender);

-- @person_suggest
-- Подсказка персонами. Сначала те, кого вводили чаще: заказчик работает
-- приходами, и одни и те же люди возвращаются в записях год за годом.
SELECT iof, place, rank, gender, uses
  FROM person_index
 WHERE iof_norm LIKE :prefix ESCAPE '\'
 ORDER BY uses DESC, iof
 LIMIT :limit;

-- @spouse_remember
-- Кто чья жена. Заполняется, когда в записи о рождении есть и отец, и мать.
INSERT INTO spouse_index (husband_norm, wife_iof, wife_place, wife_rank, uses, last_used_at)
VALUES (:husband_norm, :wife_iof, :wife_place, :wife_rank, 1, datetime('now'))
ON CONFLICT(husband_norm, wife_iof) DO UPDATE SET
    uses = uses + 1, last_used_at = datetime('now'),
    wife_place = coalesce(excluded.wife_place, wife_place),
    wife_rank = coalesce(excluded.wife_rank, wife_rank);

-- @spouse_lookup
-- Жена по мужу. Заказчик: «как только ты выбираешь существующую персону,
-- то и НП, и его звание, и все данные его жены тут же должны быть заполнены».
SELECT wife_iof, wife_place, wife_rank, uses
  FROM spouse_index
 WHERE husband_norm = :husband_norm
 ORDER BY uses DESC
 LIMIT 1;
