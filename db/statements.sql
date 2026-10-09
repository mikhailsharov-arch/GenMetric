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
    uyezd = excluded.uyezd, guberniya = excluded.guberniya,
    -- Год дела пустым не затирается: экран «Дело» мог быть открыт до первой
    -- записи и прислать прежний пустой год, а дело за это время стало делом
    -- 1897 года. Иначе следующий год забрал бы это дело вместе с записями
    -- прошлого (ревьюер 02.10.2026).
    year = coalesce(excluded.year, mk_case.year),
    parish_key = excluded.parish_key, indexer = excluded.indexer,
    updated_at = datetime('now');

-- @case_current
-- Дело, с которым сейчас работают. С 02.10.2026 дело — на год книги: у
-- каждого года своя строка mk_case (в Excel Романа 1889 год — «Д.11», 1890 —
-- «Д.12»), а экран «Дело» остаётся одной формой (Роман 02.10: «менять эту
-- логику не нужно»). :year задан — дело этого года; иначе — дело последней
-- записи; записей нет — последнее сохранённое.
SELECT id, archive, fond, opis, delo, church, village, uyezd, guberniya, year, indexer
  FROM mk_case
 WHERE id = coalesce((SELECT c.id FROM mk_case c WHERE c.year = :year ORDER BY c.id LIMIT 1),
                     (SELECT e.case_id FROM entry e ORDER BY e.id DESC LIMIT 1),
                     (SELECT c.id FROM mk_case c ORDER BY c.updated_at DESC, c.id DESC LIMIT 1));

-- @case_years
-- Годы, у которых есть дело, и сколько записей в каждом — для выбора года
-- на экране «Дело» (поправить реквизиты прошлого года).
SELECT c.year, (SELECT count(*) FROM entry e WHERE e.case_id = c.id) AS entries
  FROM mk_case c WHERE c.year IS NOT NULL ORDER BY c.year;

-- @case_for_year
SELECT id FROM mk_case WHERE year = :year ORDER BY id LIMIT 1;

-- @case_adopt_year
-- Дело без года (первое дело новой базы: год набирают на форме, не в деле)
-- становится делом первого встреченного года.
UPDATE mk_case SET year = :year
 WHERE id = (SELECT id FROM mk_case WHERE year IS NULL ORDER BY id LIMIT 1);

-- @case_copy_for_year
-- Новый год книги — новое дело копией того, с которым работали: приход тот
-- же, а фонд, опись и дело человек поправит (программа напомнит полосой).
INSERT INTO mk_case (archive, fond, opis, delo, church, village, uyezd, guberniya,
                     year, parish_key, indexer, updated_at)
SELECT archive, fond, opis, delo, church, village, uyezd, guberniya,
       :year, parish_key, indexer, datetime('now')
  FROM mk_case
 ORDER BY (id = (SELECT e.case_id FROM entry e ORDER BY e.id DESC LIMIT 1)) DESC, updated_at DESC, id DESC
 LIMIT 1;

-- @case_parish_key
-- Ключ прихода для частот подсказок: дела :id, а нет такого — первого дела.
SELECT parish_key FROM mk_case ORDER BY (id = :id) DESC, id LIMIT 1;

-- @case_or_first
-- Дело :id, а если его убрали («Убрать дело этого года») — первое дело
-- прихода. Для записи без года: года нет, дело ей назначает форма. Вторым
-- значением — год найденного дела: форма скажет, куда легла запись.
SELECT id, year FROM mk_case ORDER BY (id = :id) DESC, id LIMIT 1;

-- @case_new_id
SELECT coalesce(max(id), 0) + 1 FROM mk_case;

-- @case_year_of
SELECT year FROM mk_case WHERE id = :id;

-- @case_brief
-- Реквизиты дела одной строкой — для вопроса «заменить реквизиты дела N года?».
SELECT trim(coalesce('Ф.' || nullif(trim(fond), ''), '') || coalesce(' Оп.' || nullif(trim(opis), ''), '')
            || coalesce(' Д.' || nullif(trim(delo), ''), ''))
  FROM mk_case WHERE id = :id;

-- @case_delete_empty
-- Убрать дело года без единой записи (опечатка года; год, из которого ушла
-- последняя запись). Дело с записями и последнее дело прихода не убираются.
DELETE FROM mk_case
 WHERE year = :year
   AND NOT EXISTS (SELECT 1 FROM entry e WHERE e.case_id = mk_case.id)
   AND (SELECT count(*) FROM mk_case) > 1;

-- @case_spread_parish
-- Церковь, село, уезд, губерния и индексатор — свойства прихода, не года:
-- правка на экране «Дело» расходится на дела всех лет. Архив — туда, где пуст.
UPDATE mk_case
   SET church = :church, village = :village, uyezd = :uyezd, guberniya = :guberniya,
       parish_key = :parish_key, indexer = :indexer,
       archive = coalesce(nullif(trim(archive), ''), :archive)
 WHERE id <> :id;

-- @entry_insert
INSERT INTO entry (case_id, section, page, no_male, no_female,
                   event_day, event_month, event_year,
                   rite_day, rite_month, rite_year, note, uncertain, created_by)
VALUES (:case_id, :section, :page, :no_male, :no_female,
        :event_day, :event_month, :event_year,
        :rite_day, :rite_month, :rite_year, :note, :uncertain, :created_by);

-- @entry_update
UPDATE entry SET case_id = :case_id, page = :page, no_male = :no_male, no_female = :no_female,
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
     birth_year_from, birth_year_to, age_years, marriage_order, kinship,
     age_months, age_weeks, age_days, age_text, death_cause)
VALUES (:entry_id, :role_code, :sort_order, :surname, :first_name, :patronymic,
        :surname_modern, :first_name_modern, :patronymic_modern, :maiden_surname,
        :gender, :rank, :confession, :place_id, :note, :uncertain,
        :birth_year_from, :birth_year_to, :age_years, :marriage_order, :kinship,
        :age_months, :age_weeks, :age_days, :age_text, :death_cause);

-- @place_find
-- Если строк с одним названием несколько («двойник» без подробностей рядом
-- с пунктом поставки), запись ссылается на самую полную — иначе в выгрузке
-- у персоны не было бы полного места (ревьюер #40, Роман 01.10.2026).
-- Правило «лучшей строки» одно на всю программу (place_find, place_get,
-- выгрузка, migrate.sql, parish_sync.sql): сначала строка с полным местом,
-- затем с типом, затем не из архива, затем самая ранняя.
SELECT id, name FROM place WHERE name_norm = :name_norm
 ORDER BY (full_location IS NULL OR trim(full_location) = ''),
          (np_type IS NULL OR trim(np_type) = ''), (origin = 'archive'), id LIMIT 1;

-- @place_insert
-- Населённый пункт заводится по первому упоминанию — запасной путь, если
-- карточка (place_save) почему-то не открылась и НП дошёл до сохранения
-- записи неизвестным. Подробности тогда пусты, дозаполняются позже.
INSERT INTO place (name, name_norm, origin) VALUES (:name, :name_norm, 'user');

-- @place_save
-- Карточка населённого пункта при первом вводе (Роман, приоритет 2 от
-- 23.09.2026): губерния и уезд по умолчанию из дела, тип, волость, ссылка
-- на Familio. short/full_location собираются здесь же — как в place.csv
-- из Excel, чтобы выгрузка не различала свои и перенесённые места.
-- :name — название в программе, у деревни-тёзки с комментарием:
-- «Хмельничное (Столпино)»; :clean — чистое название, из него собираются
-- краткое и полное место: в выгрузки комментарий не идёт (Роман 06.10.2026).
INSERT INTO place (name, name_norm, np_type, guberniya, uyezd, volost,
                   short_location, full_location, familio_url, origin, updated_at, comment)
VALUES (:name, :name_norm, :np_type, :guberniya, :uyezd, :volost,
        trim(coalesce(:np_type, '') || ' ' || :clean),
        trim(coalesce(:np_type, '') || ' ' || :clean)
          || CASE WHEN :volost    IS NULL OR :volost    = '' THEN '' ELSE ', ' || :volost    || ' волость'  END
          || CASE WHEN :uyezd     IS NULL OR :uyezd     = '' THEN '' ELSE ', ' || :uyezd     || ' уезд'     END
          || CASE WHEN :guberniya IS NULL OR :guberniya = '' THEN '' ELSE ', ' || :guberniya || ' губерния' END,
        :familio_url, 'user', strftime('%Y-%m-%d %H:%M:%f', 'now'), :comment);

-- @place_get
-- Карточка известного пункта на правку (Роман 24.09.2026: «должна быть
-- возможность отредактировать НП, вдруг при вводе пользователь совершил ошибку»).
-- Порядок: если строк с одним названием несколько (поставка и архив), берётся
-- самая полная — не из архива, самая ранняя (техдолг, ревьюер 24.09.2026).
SELECT id, name, np_type, guberniya, uyezd, volost, familio_url, origin, coalesce(comment, '')
  FROM place WHERE name_norm = :name_norm
 ORDER BY (full_location IS NULL OR trim(full_location) = ''),
          (np_type IS NULL OR trim(np_type) = ''), (origin = 'archive'), id LIMIT 1;

-- @place_update
-- Правка карточки, включая название (Роман 25.09.2026: «да, вдруг
-- пользователь допустил ошибку в названии»). Записи ссылаются на пункт по id
-- и получают новое название сами; short/full_location пересобираются.
UPDATE place
   SET name = :name, name_norm = :name_norm,
       np_type = :np_type, guberniya = :guberniya, uyezd = :uyezd, volost = :volost,
       familio_url = :familio_url, updated_at = strftime('%Y-%m-%d %H:%M:%f', 'now'),
       comment = :comment,
       short_location = trim(coalesce(:np_type, '') || ' ' || :clean),
       full_location = trim(coalesce(:np_type, '') || ' ' || :clean)
          || CASE WHEN :volost    IS NULL OR :volost    = '' THEN '' ELSE ', ' || :volost    || ' волость'  END
          || CASE WHEN :uyezd     IS NULL OR :uyezd     = '' THEN '' ELSE ', ' || :uyezd     || ' уезд'     END
          || CASE WHEN :guberniya IS NULL OR :guberniya = '' THEN '' ELSE ', ' || :guberniya || ' губерния' END
 WHERE id = :id;

-- @place_name_taken
-- Другой пункт с тем же названием — переименование в него запрещено.
SELECT name FROM place WHERE name_norm = :name_norm AND id <> :id LIMIT 1;

-- @place_renamed_remember
-- REPLACE: повторное переименование того же названия освежает время — по
-- нему сверка приходов понимает, что было позже: переименование или карточка.
INSERT OR REPLACE INTO place_renamed (old_norm, renamed_at) VALUES (:old_norm, strftime('%Y-%m-%d %H:%M:%f', 'now'));

-- @place_rename_persons_merge
-- Переименование в название, под которым этот же человек уже запомнен:
-- частоты складываются в строку с новым названием (техдолг после #36 —
-- раньше строка со старым названием просто оставалась, частоты терялись).
UPDATE person_index
   SET uses = uses + (SELECT sum(p2.uses) FROM person_index p2
                       WHERE p2.iof = person_index.iof AND p2.place = :old_name
                         AND p2.rank IS person_index.rank),
       last_used_at = nullif(max(coalesce(last_used_at, ''),
                          coalesce((SELECT max(p2.last_used_at) FROM person_index p2
                                     WHERE p2.iof = person_index.iof AND p2.place = :old_name
                                       AND p2.rank IS person_index.rank), '')), '')
 WHERE place = :name
   AND EXISTS (SELECT 1 FROM person_index p2
                WHERE p2.iof = person_index.iof AND p2.place = :old_name
                  AND p2.rank IS person_index.rank);

-- @place_rename_persons_drop
-- …и строка со старым названием уходит: её частоты уже в новой.
DELETE FROM person_index
 WHERE place = :old_name
   AND EXISTS (SELECT 1 FROM person_index p2
                WHERE p2.iof = person_index.iof AND p2.place = :name
                  AND p2.rank IS person_index.rank);

-- @place_rename_persons
-- Память подсказок хранит название текстом: персоны с местом, жёны, частоты
-- (три блока ниже). Без этого выбор персоны подставлял бы старое название,
-- которого в справочнике уже нет, и открывал карточку нового пункта.
UPDATE person_index SET place = :name WHERE place = :old_name
   AND NOT EXISTS (SELECT 1 FROM person_index p2
                    WHERE p2.iof = person_index.iof AND p2.place = :name
                      AND p2.rank IS person_index.rank);

-- @place_rename_spouses
UPDATE spouse_index SET wife_place = :name WHERE wife_place = :old_name;

-- @place_rename_usage_merge
-- Частоты места: то же слияние, что у персон (ключ — kind, scope, scope_key).
UPDATE usage_stat
   SET count = count + (SELECT u2.count FROM usage_stat u2
                         WHERE u2.kind = 'place' AND u2.value = :old_name
                           AND u2.scope = usage_stat.scope AND u2.scope_key = usage_stat.scope_key),
       last_used_at = nullif(max(coalesce(last_used_at, ''),
                          coalesce((SELECT u2.last_used_at FROM usage_stat u2
                                     WHERE u2.kind = 'place' AND u2.value = :old_name
                                       AND u2.scope = usage_stat.scope AND u2.scope_key = usage_stat.scope_key), '')), '')
 WHERE kind = 'place' AND value = :name
   AND EXISTS (SELECT 1 FROM usage_stat u2
                WHERE u2.kind = 'place' AND u2.value = :old_name
                  AND u2.scope = usage_stat.scope AND u2.scope_key = usage_stat.scope_key);

-- @place_rename_usage_drop
DELETE FROM usage_stat
 WHERE kind = 'place' AND value = :old_name
   AND EXISTS (SELECT 1 FROM usage_stat u2
                WHERE u2.kind = 'place' AND u2.value = :name
                  AND u2.scope = usage_stat.scope AND u2.scope_key = usage_stat.scope_key);

-- @place_rename_usage
UPDATE OR IGNORE usage_stat SET value = :name, value_norm = :name_norm
 WHERE kind = 'place' AND value = :old_name;

-- @place_import_fill
-- Импорт из Excel-индексатора: подробности пункта с листа «НП» — пункту без
-- единой подробности. С 08.10.2026 пунктов в поставке нет, и без этого шага
-- пункты импортированного прихода оставались бы одними названиями: в
-- выгрузке Familio не было бы ни типа, ни уезда, ни ссылки. Заполненное или
-- поправленное человеком не трогается (то же условие, что было у
-- дозаполнения из поставки в migrate.sql).
UPDATE place
   SET np_type = :np_type, guberniya = :guberniya, uyezd = :uyezd, volost = :volost,
       short_location = coalesce(:short_location, short_location),
       full_location = coalesce(:full_location, full_location), familio_url = :familio_url
 WHERE id = :id
   AND trim(coalesce(np_type, '')) = '' AND trim(coalesce(guberniya, '')) = ''
   AND trim(coalesce(uyezd, '')) = '' AND trim(coalesce(volost, '')) = ''
   AND trim(coalesce(familio_url, '')) = ''
   AND (trim(coalesce(short_location, '')) = '' OR short_location = name)
   AND (trim(coalesce(full_location, '')) = '' OR full_location = name);

-- @place_import_kept
-- Пункт листа «НП» уже известен и со своими подробностями (пришёл из общего
-- файла приходов — одноимённая деревня другого прихода): подробности файла
-- его не меняют, но молчать об этом нельзя — строка идёт в отчёт импорта.
-- Здесь — что у пункта стоит сейчас, для сравнения с файлом.
SELECT coalesce(np_type, ''), coalesce(guberniya, ''), coalesce(uyezd, ''), coalesce(volost, '')
  FROM place WHERE id = :id;

-- @place_import_insert
-- …и пункт листа «НП», которого в записях нет: справочник индексатора
-- переносится целиком — это деревни того же прихода, они понадобятся.
INSERT INTO place (name, name_norm, np_type, guberniya, uyezd, volost,
                   short_location, full_location, familio_url, origin)
VALUES (:name, :name_norm, :np_type, :guberniya, :uyezd, :volost,
        :short_location, :full_location, :familio_url, 'user');

-- @place_commented
-- Пункты с комментарием — деревни-тёзки: набрали «Хмельничное», а в
-- справочнике «Хмельничное (Столпино)» и «Хмельничное (Нежитино)» — карточка
-- нового пункта покажет их первыми среди похожих.
SELECT name, comment FROM place WHERE trim(coalesce(comment, '')) <> '';

-- @place_names
-- Все названия для поиска похожих («Букарина» → «Бухарино»): расстояние
-- считает приложение, здесь только перечень.
SELECT name, name_norm FROM place;

-- @alias_find
SELECT target, gender FROM name_alias WHERE kind = :kind AND form_norm = :form_norm;

-- @alias_save
-- «Запомнить» в окне сверки: одно соответствие на написание, повторное
-- решение заменяет прежнее.
INSERT OR REPLACE INTO name_alias (kind, form, form_norm, target, gender)
VALUES (:kind, :form, :form_norm, :target, :gender);

-- @name_headwords
-- Имена-основы словаря для поиска похожих. Разговорные формы с base_name
-- («Марья» → «Мария») сюда не входят: соответствие ведёт к основе.
SELECT DISTINCT name, name_norm, gender FROM name_dict WHERE coalesce(base_name, '') = '';

-- @dict_name_prefix
-- Поиск в окне сверки — только по словарю (основы, без разговорных форм):
-- подсказка suggest_first_name смешивает словарь с набранным и с архивом
-- Excel, а цель соответствия обязана быть словарным именем, иначе оно так
-- и останется «не сверенным» (проверяющий 23.09.2026).
SELECT DISTINCT name AS form, gender FROM name_dict
 WHERE coalesce(base_name, '') = '' AND name_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender = :gender)
 ORDER BY name LIMIT :limit;

-- @dict_patr_prefix
SELECT DISTINCT form, gender FROM name_form
 WHERE kind IN ('patr_old_m', 'patr_old_f', 'patr_m', 'patr_f')
   AND form_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender = :gender)
 ORDER BY form LIMIT :limit;

-- @patr_forms
-- Формы отчеств для поиска похожих: старые («Иванов») и современные
-- («Иванович»), по полу.
SELECT DISTINCT form, form_norm, gender FROM name_form
 WHERE kind IN ('patr_old_m', 'patr_old_f', 'patr_m', 'patr_f');

-- @lookup_by_norm
-- Значение перечня по ключу — для званий в старой орфографии (main.rs remember).
SELECT value FROM lookup WHERE kind = :kind AND value_norm = :value_norm
 ORDER BY origin = 'user', id LIMIT 1;

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

-- @surname_counts
-- Фамилии записей прихода с числом упоминаний — чтобы подсказка фамилий
-- (третье слово ИОФ) знала и то, что набрано или импортировано до неё.
SELECT trim(surname) AS surname, count(*) AS n
  FROM person_mention
 WHERE trim(coalesce(surname, '')) <> ''
   AND role_code NOT LIKE 'clergy%'   -- причт повторяется в каждой записи
 GROUP BY trim(surname);

-- @usage_set
-- Частота, посчитанная разом (первое заполнение), а не наращиваемая.
INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)
VALUES (:kind, :scope, :scope_key, :value, :value_norm, :count, datetime('now'))
ON CONFLICT(kind, scope, scope_key, value) DO UPDATE SET count = excluded.count;

-- @confession_counts
-- Вероисповедания записей прихода с числом упоминаний: частоты по ним до
-- 06.10.2026 не велись, и подсказка шла по алфавиту, а не «самое частое —
-- первым» (Роман 06.10.2026: «абсолютно во всех полях»).
SELECT trim(confession) AS value, count(*) AS n
  FROM person_mention
 WHERE trim(coalesce(confession, '')) <> ''
 GROUP BY trim(confession);

-- @place_field_counts
-- То же для полей карточки пункта: тип, губерния, уезд, волость — по пунктам,
-- на которые ссылаются записи, и по пунктам, заведённым человеком.
-- Без четырёх значений из строк шаблона Excel («Петровская», «Лодзинский»,
-- «Галический», «Котельская»): Роман просил их убрать (06.10.2026), а у него
-- самого они заняты записями и остаются — подсчёт частот поднял бы их в
-- верхний ярус подсказки (ревьюер 06.10.2026).
WITH used AS (
    SELECT np_type, guberniya, uyezd, volost FROM place
     WHERE origin = 'user' OR EXISTS (SELECT 1 FROM person_mention m WHERE m.place_id = place.id)
), flat AS (
    SELECT 'np_type' AS kind, trim(np_type) AS value FROM used
    UNION ALL SELECT 'guberniya', trim(guberniya) FROM used
    UNION ALL SELECT 'uyezd', trim(uyezd) FROM used
    UNION ALL SELECT 'volost', trim(volost) FROM used
)
SELECT kind, value, count(*) AS n FROM flat
 WHERE coalesce(value, '') <> ''
   AND NOT (kind = 'guberniya' AND value = 'Петровская')
   AND NOT (kind = 'uyezd' AND value IN ('Лодзинский', 'Галический'))
   AND NOT (kind = 'volost' AND value = 'Котельская')
 GROUP BY kind, value;

-- @usage_raise
-- Первое заполнение частот поверх уже имеющихся: у уезда и губернии частоты
-- есть от «Сохранить дело», и подсчёт по пунктам их не должен ни пропустить,
-- ни занизить — берётся большее.
INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)
VALUES (:kind, :scope, :scope_key, :value, :value_norm, :count, datetime('now'))
ON CONFLICT(kind, scope, scope_key, value) DO UPDATE SET count = max(count, excluded.count);

-- @setting_get
SELECT value FROM setting WHERE key = :key;

-- @setting_put
INSERT INTO setting (key, value) VALUES (:key, :value)
ON CONFLICT(key) DO UPDATE SET value = excluded.value;

-- @place_last_volost
-- Волость последнего пункта, заведённого человеком, — в карточку следующего
-- (Роман 06.10.2026: «предзаполнялось значением от последнего заведенного НП»).
SELECT coalesce(volost, '') FROM place WHERE origin = 'user' ORDER BY id DESC LIMIT 1;

-- @rank_default
-- Самое частое звание роли и пола в приходе — в пустое поле звания (Роман
-- 06.10.2026, вариант А). По упоминаниям роли, а не по общим частотам
-- перечня: иначе восприемнице досталось бы «законная жена его» — матерей в
-- приходе больше всех. :role — шаблон LIKE («godparent%», «father»).
-- Родственники жениха и невесты (Роман 09.10.2026) — шаблон «%_relative»;
-- родственник умершего под него тоже подходит, а звание у него другое
-- («отец», «муж» живых против родителей брачующихся) — исключён явно.
SELECT trim(rank) FROM person_mention
 WHERE role_code LIKE :role AND role_code <> 'deceased_relative'
   AND trim(coalesce(rank, '')) <> ''
   AND (:gender IS NULL OR gender = :gender)
 GROUP BY trim(rank)
 ORDER BY count(*) DESC, max(id) DESC
 LIMIT 1;

-- @surname_known
-- Была ли такая фамилия в приходе — для отметки «такой фамилии ещё не было»
-- (Роман 06.10.2026). Спрашивают сразу обе формы, мужскую и женскую: :a, :b.
-- И с дореформенным «ъ» на конце: в частотах фамилия лежит как набрана.
-- :c — вторая возможная мужская форма («Толстая» — «Толстый» или «Толстой»).
SELECT EXISTS (SELECT 1 FROM lookup WHERE kind = 'surname' AND value_norm IN (:a, :b, :c, :a || 'ъ'))
    OR EXISTS (SELECT 1 FROM usage_stat WHERE kind = 'surname' AND scope = 'global' AND scope_key = ''
                                          AND value_norm IN (:a, :b, :c, :a || 'ъ'));

-- @usage_has
SELECT EXISTS (SELECT 1 FROM usage_stat WHERE kind = :kind);

-- @entry_list
-- Список набранных записей дела: номер, дата, имя ребёнка — чтобы вернуться
-- и поправить.
SELECT e.id, e.page, e.no_male, e.no_female,
       e.event_day, e.event_month, e.event_year, e.rite_month,
       (SELECT trim(coalesce(m.first_name, '') || ' ' || coalesce(m.patronymic, '')
                    || ' ' || coalesce(m.surname, ''))
          FROM person_mention m
         WHERE m.entry_id = e.id AND m.role_code = 'child') AS child,
       -- Отец в строке: правка отца иначе в списке не видна (Роман 23.09.2026).
       (SELECT trim(coalesce(m.first_name, '') || ' ' || coalesce(m.patronymic, '')
                    || ' ' || coalesce(m.surname, ''))
          FROM person_mention m
         WHERE m.entry_id = e.id AND m.role_code = 'father') AS father,
       -- Причт без имени (сборки 21–22.09) — пометить в списке, чтобы найти и поправить.
       EXISTS (SELECT 1 FROM person_mention c
                WHERE c.entry_id = e.id AND c.role_code IN ('clergy1','clergy2','clergy3')
                  AND c.first_name IS NULL AND c.surname IS NULL AND c.rank IS NOT NULL) AS clergy_noname,
       -- Браки (25.09.2026): жених и невеста в строке списка.
       (SELECT trim(coalesce(m.first_name, '') || ' ' || coalesce(m.patronymic, '')
                    || ' ' || coalesce(m.surname, ''))
          FROM person_mention m
         WHERE m.entry_id = e.id AND m.role_code = 'groom') AS groom,
       (SELECT trim(coalesce(m.first_name, '') || ' ' || coalesce(m.patronymic, '')
                    || ' ' || coalesce(m.surname, ''))
          FROM person_mention m
         WHERE m.entry_id = e.id AND m.role_code = 'bride') AS bride,
       -- Смерти (27.09.2026): умерший в строке списка.
       (SELECT trim(coalesce(m.first_name, '') || ' ' || coalesce(m.patronymic, '')
                    || ' ' || coalesce(m.surname, ''))
          FROM person_mention m
         WHERE m.entry_id = e.id AND m.role_code = 'deceased') AS deceased,
       -- Год книги для «продолжить с места»: у записи «событие в предыдущем
       -- году» он равен году обряда (сборка #38).
       e.rite_year
  FROM entry e
 -- Записи года книги (:year), а не всего прихода: после импорта из Excel в
 -- списке иначе была бы тысяча строк. :last = 1 — одна последняя запись
 -- раздела любого года, для «продолжить с места».
 WHERE e.section = :section
   AND (:last = 1 OR coalesce(e.rite_year, e.event_year) IS :year)
 ORDER BY e.id DESC
 LIMIT CASE WHEN :last = 1 THEN 1 ELSE -1 END;

-- @entry_get
-- Запись целиком — для правки уже сохранённого (заказчик 22.09.2026).
SELECT id, page, no_male, no_female, event_day, event_month, event_year,
       rite_day, rite_month, rite_year, note
  FROM entry
 WHERE id = :id;

-- @mentions_of_entry
-- Персоны записи для формы: НП — названием, как его набирают, а не id.
SELECT m.role_code, m.sort_order, m.surname, m.first_name, m.patronymic,
       m.surname_modern, m.first_name_modern, m.patronymic_modern, m.maiden_surname,
       m.gender, m.rank, m.confession,
       (SELECT p.name FROM place p WHERE p.id = m.place_id) AS place,
       m.note, m.uncertain, m.age_years, m.marriage_order, m.kinship,
       m.age_text, m.death_cause
  FROM person_mention m
 WHERE m.entry_id = :entry_id
 ORDER BY m.sort_order;

-- @last_clergy
-- Причт последней записи дела — чтобы после перезапуска форма продолжала
-- с ним, как со страницей и счётом (заказчик 21.09.2026: «надо сделать,
-- чтобы церковнослужители также сохранялись»).
-- :section = 0 — последняя запись дела в любом разделе: причт с 27.09.2026
-- общий для рождений, браков и смертей.
SELECT m.role_code,
       -- Без отчества между именем и фамилией остался бы двойной пробел.
       trim(coalesce(m.first_name, '')
            || CASE WHEN m.patronymic IS NULL THEN '' ELSE ' ' || m.patronymic END
            || CASE WHEN m.surname IS NULL THEN '' ELSE ' ' || m.surname END) AS iof,
       m.rank, m.note
  FROM person_mention m
 WHERE m.entry_id = (SELECT max(e.id) FROM entry e WHERE (:section = 0 OR e.section = :section))
   AND m.role_code IN ('clergy1', 'clergy2', 'clergy3')
 ORDER BY m.sort_order;

-- @suggest_ranked
-- ПОДСКАЗКИ. Раньше эти запросы жили прямо в коде на Rust — и именно там
-- спрятался пункт 1 отчёта от 24.08.2026: поле НП искало населённые пункты
-- в перечнях lookup, где их нет и быть не может. Проверка это ловит только
-- если запрос лежит здесь, поэтому он лежит здесь.
--
-- Устроено из двух частей: эта обёртка и один из источников словаря ниже.
-- Источник подставляется на место фигурных скобок в теле запроса — одинаково
-- приложением и db/test_suggest.py. Писать это место здесь, в комментарии,
-- нельзя: подстановка заменит и его, и запрос развалится.
--
-- Порядок выдачи: самое частое в приходе, затем во всей базе, затем словарь;
-- внутри группы — по убыванию частоты. Роман 05.10.2026: «на самом верху
-- списка должно предлагаться самое часто встречающееся в базе значение».
-- Яруса «текущее дело» больше нет (требование А-1 его вводило): дело теперь
-- на год книги, и каждый новый год этот ярус начинался бы с нуля. Строки
-- охвата «дело» считаются приходскими — у них счёт не больше приходского.
WITH ranked AS (
    SELECT value,
           CASE scope WHEN 'global' THEN 2 ELSE 1 END AS tier,
           count
      FROM usage_stat
     WHERE kind = :kind AND value_norm LIKE :prefix ESCAPE '\'
     {usage_gender}
    UNION ALL
    {dict}
)
SELECT value, min(tier) AS tier, max(count) AS cnt
  FROM ranked GROUP BY value ORDER BY tier, cnt DESC, value LIMIT :limit;

-- @usage_gender_filter
-- Подставляется в suggest_ranked (место в фигурных скобках) для имён и отчеств.
-- Писать имя места здесь нельзя — блок попадает в SQL целиком. Частоты (usage_stat)
-- пола не знают, а архив из Excel принёс сотни имён обоих полов — и матери
-- снова предлагались все имена (заказчик 15.09.2026), хотя словарная ветка
-- уже фильтровалась. Сверяем со словарём; имя, которого в словаре нет,
-- показываем обоим — лучше лишнее, чем спрятать настоящее.
AND (:gender IS NULL
     OR NOT EXISTS (SELECT 1 FROM name_form f WHERE f.form_norm = usage_stat.value_norm)
     OR EXISTS (SELECT 1 FROM name_form f
                 WHERE f.form_norm = usage_stat.value_norm AND f.gender = :gender))

-- @surname_gender_filter
-- Фамилии по полу персоны (06.10.2026): женщине не предлагать «Томилин»,
-- мужчине — «Томилина». Словаря фамилий нет, поэтому фамилия другого рода
-- прячется, только когда в приходе известна её пара: «Томилин» у женщины —
-- если есть «Томилина». Без пары фамилия остаётся: «Сова» и «Калина» —
-- мужские фамилии, «Палий» — женская, а если набран один «Томилин», женщине
-- его лучше показать, чем промолчать (ревьюер 06.10.2026). Дореформенный «ъ»
-- на конце учтён.
-- Подставляется и в ветку частот (место usage_gender в suggest_ranked), и в
-- словарную ветку suggest_surname. Слово outer в фигурных скобках сборка
-- запроса заменяет именем таблицы внешней строки (usage_stat или lookup):
-- во вложенном запросе та же таблица названа pair, и без имени value_norm
-- означал бы её колонку. Пара ищется по индексам (kind, value_norm) —
-- первый вариант с объединением таблиц без индекса на 12 000 фамилий думал
-- 25 секунд на букву (проверяющий 06.10.2026).
AND (:gender IS NULL OR :gender NOT IN ('М', 'Ж')
     OR (:gender = 'Ж' AND NOT (
            ((rtrim({outer}.value_norm, 'ъ') LIKE '%ов' OR rtrim({outer}.value_norm, 'ъ') LIKE '%ев' OR rtrim({outer}.value_norm, 'ъ') LIKE '%ин' OR rtrim({outer}.value_norm, 'ъ') LIKE '%ын')
             AND (EXISTS (SELECT 1 FROM lookup pair WHERE pair.kind = 'surname' AND pair.value_norm IN (rtrim({outer}.value_norm, 'ъ') || 'а'))
                      OR EXISTS (SELECT 1 FROM usage_stat pair WHERE pair.kind = 'surname' AND pair.scope = 'global'
                                  AND pair.scope_key = '' AND pair.value_norm IN (rtrim({outer}.value_norm, 'ъ') || 'а'))))
         OR (({outer}.value_norm LIKE '%ий' OR {outer}.value_norm LIKE '%ой' OR {outer}.value_norm LIKE '%ый')
             AND (EXISTS (SELECT 1 FROM lookup pair WHERE pair.kind = 'surname' AND pair.value_norm IN (substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ая'))
                      OR EXISTS (SELECT 1 FROM usage_stat pair WHERE pair.kind = 'surname' AND pair.scope = 'global'
                                  AND pair.scope_key = '' AND pair.value_norm IN (substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ая'))))))
     OR (:gender = 'М' AND NOT (
            (({outer}.value_norm LIKE '%ова' OR {outer}.value_norm LIKE '%ева' OR {outer}.value_norm LIKE '%ина' OR {outer}.value_norm LIKE '%ына')
             AND (EXISTS (SELECT 1 FROM lookup pair WHERE pair.kind = 'surname' AND pair.value_norm IN (substr({outer}.value_norm, 1, length({outer}.value_norm) - 1), substr({outer}.value_norm, 1, length({outer}.value_norm) - 1) || 'ъ'))
                      OR EXISTS (SELECT 1 FROM usage_stat pair WHERE pair.kind = 'surname' AND pair.scope = 'global'
                                  AND pair.scope_key = '' AND pair.value_norm IN (substr({outer}.value_norm, 1, length({outer}.value_norm) - 1), substr({outer}.value_norm, 1, length({outer}.value_norm) - 1) || 'ъ'))))
         OR ({outer}.value_norm LIKE '%ая'
             AND (EXISTS (SELECT 1 FROM lookup pair WHERE pair.kind = 'surname' AND pair.value_norm IN (substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ий', substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ой', substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ый'))
                      OR EXISTS (SELECT 1 FROM usage_stat pair WHERE pair.kind = 'surname' AND pair.scope = 'global'
                                  AND pair.scope_key = '' AND pair.value_norm IN (substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ий', substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ой', substr({outer}.value_norm, 1, length({outer}.value_norm) - 2) || 'ый')))))))

-- @suggest_surname
-- Словарная ветка для фамилий — перечень lookup (его пополняет сохранение
-- записи). К запросу дописывается surname_gender_filter.
SELECT value, 4, 0 FROM lookup
 WHERE kind = :kind AND value_norm LIKE :prefix ESCAPE '\'

-- @suggest_first_name
-- Имя по полу роли: матери — женские, отцу — мужские. Заказчик 13.09.2026:
-- «при вводе ИОФ матери подставляет мужские имена». Пол неизвестен — обе.
SELECT form, 4, 0 FROM name_form
 WHERE kind IN ('name','variant') AND form_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender = :gender)
-- Написания, которые человек связал со словарём в окне сверки («Пескарь» →
-- «Кесарь»): Роман 05.10.2026 — «пополнять локальный список автокомплита …
-- теми оригинальными написаниями, которые пользователь уже вводил и
-- связывал». И «новые имена» (без цели) — тоже.
UNION ALL
SELECT form, 3, 0 FROM name_alias
 WHERE kind = 'name' AND form_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender IS NULL OR gender = :gender)

-- @suggest_patronymic
-- Отчество мужчины и отчество женщины — разные формы одного имени. Пол берётся
-- из роли персоны: отец всегда мужчина, мать всегда женщина. Если пол
-- неизвестен (ребёнок, восприемник), показываются обе формы.
SELECT form, 4, 0 FROM name_form
 WHERE kind LIKE 'patr%' AND form_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender = :gender)
-- Связанные человеком отчества; «это не отчество» (цель пуста) не предлагаем.
-- По полу — как словарные: у невесты предлагались и мужские (Роман
-- 06.10.2026). Пола у соответствия отчества нет — он виден по окончанию
-- написания: женское отчество кончается на «а» («Иванова», «Ивановна»).
UNION ALL
SELECT form, 3, 0 FROM name_alias
 WHERE kind = 'patr' AND target IS NOT NULL AND form_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR :gender NOT IN ('М', 'Ж')
        OR (:gender = 'Ж') = (rtrim(form_norm, 'ъ') LIKE '%а'))

-- @suggest_place
-- Населённые пункты живут в своей таблице, а не в плоских перечнях: у них
-- губерния, уезд, волость и ссылка на Familio.
SELECT name, 4, 0 FROM place WHERE name_norm LIKE :prefix ESCAPE '\'

-- @suggest_lookup
SELECT value, 4, 0 FROM lookup
 WHERE kind = :kind AND value_norm LIKE :prefix ESCAPE '\'

-- @person_remember
-- Запоминает персону целиком: ИОФ вместе с населённым пунктом и званием.
-- Ради этого всё и делается — выбор строки заполняет три поля разом.
-- Пустые место и звание — пустой строкой, не NULL: в UNIQUE у SQLite NULL
-- не равен NULL, и персона без места задваивалась при каждом сохранении
-- (техдолг В6, 27.09.2026). Наружу пустое отдаётся снова как NULL.
INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses, last_used_at)
VALUES (:iof, :iof_norm, coalesce(:place, ''), coalesce(:rank, ''), :gender, 1, datetime('now'))
ON CONFLICT(iof, place, rank) DO UPDATE SET
    uses = uses + 1, last_used_at = datetime('now'),
    gender = coalesce(excluded.gender, gender);

-- @person_suggest
-- Подсказка персонами. Сначала те, кого вводили чаще: заказчик работает
-- приходами, и одни и те же люди возвращаются в записях год за годом.
-- Персоны — тоже по полу роли. Пол у персоны может быть не записан (старые
-- записи), такие показываются всем: лучше лишняя строка, чем потерянный человек.
-- Одна персона — одна строка (Роман 06.10.2026): «при совпадении ИОФ и НП
-- программа должна сохранять последнее введенное звание и предлагать… только
-- одну актуальную запись». Память не переписывается: строки с разными
-- званиями сворачиваются здесь, звание — последнее набранное (непустое, если
-- такое есть), частота — сумма. Оконными функциями, без коррелированного
-- подзапроса: он на большой памяти персон стоил бы секунд на букву.
-- Строка без НП прячется, когда у того же ИОФ есть строка с НП (Роман
-- 09.10.2026: «показывается дважды: одна запись с заполненным НП, вторая —
-- без. По логике они должны были слиться»). Без НП человек попадает в память
-- причтом — у причта НП нет. Частота спрятанной строки прибавляется строкам
-- с НП. Память и здесь не переписывается.
SELECT iof, nullif(place, '') AS place, nullif(rank, '') AS rank, gender, uses
  FROM (SELECT p.iof, p.place, p.rank,
               max(p.gender) OVER (PARTITION BY p.iof, p.place) AS gender,
               sum(p.uses) OVER (PARTITION BY p.iof, p.place)
                 + CASE WHEN p.place <> ''
                        THEN sum(CASE WHEN p.place = '' THEN p.uses ELSE 0 END) OVER (PARTITION BY p.iof)
                        ELSE 0 END AS uses,
               max(p.place <> '') OVER (PARTITION BY p.iof) AS has_place,
               row_number() OVER (PARTITION BY p.iof, p.place
                                  ORDER BY (p.rank = ''), coalesce(p.last_used_at, '') DESC, p.id DESC) AS rn
          FROM person_index p
         WHERE p.iof_norm LIKE :prefix ESCAPE '\'
           AND (:gender IS NULL OR p.gender IS NULL OR p.gender = :gender))
 WHERE rn = 1 AND (place <> '' OR has_place = 0)
 ORDER BY uses DESC, iof
 LIMIT :limit;

-- @person_suggest_infant
-- Подсказка ИОФ умершего (Роман 28.09.2026): «в записях о смерти огромную
-- долю составляют младенцы (которые фигурировали только в записях о
-- рождении)». Первыми — те, кто есть ребёнком в записи о рождении, потом
-- остальные по частоте. ИОФ собирается так же, как person_iof в main.rs.
-- Некоррелированный IN: SQLite считает множество один раз (ревьюер #38:
-- коррелированный EXISTS давал 14–30 с на букву при 80 тыс. упоминаний).
-- Одна персона — одна строка (Роман 06.10.2026): «при совпадении ИОФ и НП
-- программа должна сохранять последнее введенное звание и предлагать… только
-- одну актуальную запись». Память не переписывается: строки с разными
-- званиями сворачиваются здесь, звание — последнее набранное (непустое, если
-- такое есть), частота — сумма. Оконными функциями, без коррелированного
-- подзапроса: он на большой памяти персон стоил бы секунд на букву.
-- Строка без НП прячется, когда у того же ИОФ есть строка с НП (Роман
-- 09.10.2026: «показывается дважды: одна запись с заполненным НП, вторая —
-- без. По логике они должны были слиться»). Без НП человек попадает в память
-- причтом — у причта НП нет. Частота спрятанной строки прибавляется строкам
-- с НП. Память и здесь не переписывается.
SELECT iof, nullif(place, '') AS place, nullif(rank, '') AS rank, gender, uses
  FROM (SELECT p.iof, p.place, p.rank,
               max(p.gender) OVER (PARTITION BY p.iof, p.place) AS gender,
               sum(p.uses) OVER (PARTITION BY p.iof, p.place)
                 + CASE WHEN p.place <> ''
                        THEN sum(CASE WHEN p.place = '' THEN p.uses ELSE 0 END) OVER (PARTITION BY p.iof)
                        ELSE 0 END AS uses,
               max(p.place <> '') OVER (PARTITION BY p.iof) AS has_place,
               row_number() OVER (PARTITION BY p.iof, p.place
                                  ORDER BY (p.rank = ''), coalesce(p.last_used_at, '') DESC, p.id DESC) AS rn
          FROM person_index p
         WHERE p.iof_norm LIKE :prefix ESCAPE '\'
           AND (:gender IS NULL OR p.gender IS NULL OR p.gender = :gender))
 WHERE rn = 1 AND (place <> '' OR has_place = 0)
 ORDER BY iof IN (SELECT trim(coalesce(nullif(trim(m.first_name), ''), '') || coalesce(' ' || nullif(trim(m.patronymic), ''), '') || coalesce(' ' || nullif(trim(m.surname), ''), '')) FROM person_mention m
                   WHERE m.role_code = 'child') DESC,
          uses DESC, iof
 LIMIT :limit;

-- @infant_suggest
-- Подсказка ИОФ умершего: каждый ребёнок из записей о рождении прихода (всех
-- лет: умерший в январе родился в прошлом году, а дело теперь на год) —
-- отдельной строкой со своим родителем, местом и датой рождения. Роман
-- 30.09.2026 на вопрос о детях с одним именем: «вариант с именем отца —
-- хорошая идея». Раньше «Мария» была одной строкой на всех Марий, а при
-- нескольких рождениях отец не подставлялся вовсе (birth_father ниже).
-- Родитель — отец, а если его в записи нет (незаконнорождённые) — мать.
-- Поиск по началу имени — через person_index (у него есть ключ iof_norm;
-- некоррелированный IN, как в person_suggest_infant). Недавно родившиеся —
-- первыми: умирают чаще всего в первые месяцы.
--
-- Роман 03.10.2026: на «Евдокия» 19 строк — искать дольше, чем набрать отца.
-- По его файлу до двух лет умирают 93 % детей, но одно окно список не
-- сокращает (медиана 6 строк), сокращает деревня (1 строка у 63 %, до трёх —
-- у 97 %). Поэтому: :year — год формы смертей, дети за 2 года до него;
-- :place — НП умершего, если уже набран: только дети этой деревни (по НП
-- отца, нет отца — матери); :name и :parent — второе слово в поле отбирает по
-- началу имени родителя («Евдокия Ив»).
WITH kid AS (
    SELECT c.entry_id, c.gender, e.id AS eid, e.event_day, e.event_month, e.event_year,
           trim(coalesce(nullif(trim(c.first_name), ''), '') || coalesce(' ' || nullif(trim(c.patronymic), ''), '') || coalesce(' ' || nullif(trim(c.surname), ''), '')) AS iof
      FROM person_mention c
      JOIN entry e ON e.id = c.entry_id AND e.section = 1
     WHERE c.role_code = 'child'
),
parent AS (
    SELECT p.entry_id, p.role_code, p.rank,
           (SELECT pl.name FROM place pl WHERE pl.id = p.place_id) AS place,
           (SELECT pl.name_norm FROM place pl WHERE pl.id = p.place_id) AS place_norm,
           trim(coalesce(nullif(trim(p.first_name), ''), '') || coalesce(' ' || nullif(trim(p.patronymic), ''), '') || coalesce(' ' || nullif(trim(p.surname), ''), '')) AS iof
      FROM person_mention p WHERE p.role_code IN ('father', 'mother')
)
SELECT k.iof, k.gender,
       CASE WHEN f.iof <> '' THEN 'отец' WHEN m.iof <> '' THEN 'мать' END AS kin,
       CASE WHEN f.iof <> '' THEN f.iof WHEN m.iof <> '' THEN m.iof END AS parent,
       CASE WHEN f.iof <> '' THEN f.place ELSE m.place END AS place,
       CASE WHEN f.iof <> '' THEN f.rank ELSE m.rank END AS rank,
       k.event_day, k.event_month, k.event_year
  FROM kid k
  LEFT JOIN parent f ON f.entry_id = k.entry_id AND f.role_code = 'father'
  LEFT JOIN parent m ON m.entry_id = k.entry_id AND m.role_code = 'mother'
 WHERE k.iof <> ''
   AND (k.iof IN (SELECT iof FROM person_index WHERE iof_norm LIKE :prefix ESCAPE '\')
        OR (:parent IS NOT NULL
            AND k.iof IN (SELECT iof FROM person_index WHERE iof_norm LIKE :name ESCAPE '\')
            AND CASE WHEN f.iof <> '' THEN f.iof ELSE m.iof END
                IN (SELECT iof FROM person_index WHERE iof_norm LIKE :parent ESCAPE '\')))
   AND (:gender IS NULL OR k.gender IS NULL OR k.gender = :gender)
   AND (:year IS NULL OR k.event_year IS NULL OR k.event_year BETWEEN :year - 2 AND :year)
   AND (:place IS NULL OR CASE WHEN f.iof <> '' THEN f.place_norm ELSE m.place_norm END = :place)
 ORDER BY k.event_year DESC, k.event_month DESC, k.event_day DESC, k.eid DESC
 LIMIT :limit;

-- @birth_father
-- Отец из записи о рождении ребёнка с этим ИОФ (Роман 28.09.2026: выбрали
-- умершего младенца — родственник заполняется его отцом). Только отец с
-- именем и только рождения не позже года формы и не раньше чем за 2 года до
-- него (:year), а если НП умершего набран — только в этой деревне (:place):
-- то же окно, что у infant_suggest. Вторая колонка —
-- сколько таких записей: если их больше одной (десятки «Марий»), отец не
-- угадывается (проверяющий #38).
SELECT trim(coalesce(nullif(trim(f.first_name), ''), '') || coalesce(' ' || nullif(trim(f.patronymic), ''), '') || coalesce(' ' || nullif(trim(f.surname), ''), '')) AS iof,
       (SELECT p.name FROM place p WHERE p.id = f.place_id) AS place,
       f.rank,
       count(*) OVER () AS births
  FROM person_mention c
  JOIN entry e ON e.id = c.entry_id AND e.section = 1
  JOIN person_mention f ON f.entry_id = c.entry_id AND f.role_code = 'father'
 WHERE c.role_code = 'child'
   AND trim(coalesce(nullif(trim(c.first_name), ''), '') || coalesce(' ' || nullif(trim(c.patronymic), ''), '') || coalesce(' ' || nullif(trim(c.surname), ''), '')) = :iof
   AND trim(coalesce(nullif(trim(f.first_name), ''), '') || coalesce(' ' || nullif(trim(f.patronymic), ''), '') || coalesce(' ' || nullif(trim(f.surname), ''), '')) <> ''
   AND (:year IS NULL OR e.event_year IS NULL OR e.event_year BETWEEN :year - 2 AND :year)
   AND (:place IS NULL OR (SELECT p.name_norm FROM place p WHERE p.id = f.place_id) = :place)
 ORDER BY e.id DESC
 LIMIT 1;

-- @review_add
-- Список на сверку после импорта (схема 9).
INSERT INTO review_item (kind, text, sheet, row, entry_id)
VALUES (:kind, :text, :sheet, :row, :entry_id);

-- @review_list
-- :done = 0 — только несделанное; 1 — всё. Раздел, год и счёт записи — чтобы
-- человек понял, о какой записи речь, не открывая её.
SELECT r.id, r.kind, r.text, r.sheet, r.row, r.entry_id, r.done,
       e.section, coalesce(e.rite_year, e.event_year) AS year, e.page,
       coalesce(e.no_male, e.no_female) AS no
  FROM review_item r LEFT JOIN entry e ON e.id = r.entry_id
 WHERE (:done = 1 OR r.done = 0)
 ORDER BY r.done, (r.kind = 'skipped') DESC, e.section, r.row, r.id;

-- @review_count
SELECT count(*) FROM review_item WHERE done = 0;

-- @review_done
UPDATE review_item SET done = :done WHERE id = :id;

-- @clergy_remember
-- Причт запоминается, чтобы его можно было выбирать, а не набирать. Заказчик
-- 27.08.2026: «если в списке его нет, то после ввода его руками он добавляется
-- в базу и появляется в списке».
-- Пустое звание — пустой строкой: NULL в UNIQUE задваивал причт на каждой
-- записи (ревьюер 27.09.2026, то же, что В6 у персон).
INSERT INTO clergy_index (iof, iof_norm, rank, uses, last_used_at)
VALUES (:iof, :iof_norm, coalesce(:rank, ''), 1, datetime('now'))
ON CONFLICT(iof, rank) DO UPDATE SET
    uses = uses + 1, last_used_at = datetime('now');

-- @clergy_list
-- Весь список целиком, без ввода первых букв: причт в приходе меняется редко,
-- за год-два это те же три человека.
SELECT iof, nullif(rank, '') AS rank, uses FROM clergy_index
 ORDER BY uses DESC, last_used_at DESC, iof
 LIMIT :limit;

-- @spouse_extend
-- Жена пришла из памяти без фамилии («Олимпиада Иванова»), форма дописала ей
-- фамилию отца — это та же жена, а не вторая: прежняя строка дополняется,
-- иначе у мужа в памяти оказывались две жены (техдолг 06.10.2026). Только
-- если строки с полным ИОФ ещё нет. Исполняется перед spouse_remember.
-- И только строка того же пункта (или без пункта): у тёзки мужа из другой
-- деревни может быть жена с тем же именем и отчеством — её строку трогать
-- нельзя (ревьюер 09.10.2026).
UPDATE spouse_index SET wife_iof = :wife_iof
 WHERE husband_norm = :husband_norm
   AND substr(:wife_iof, 1, length(wife_iof) + 1) = wife_iof || ' '
   AND instr(wife_iof, ' ') > 0
   AND coalesce(wife_place, '') IN ('', coalesce(:wife_place, ''))
   AND NOT EXISTS (SELECT 1 FROM spouse_index x
                    WHERE x.husband_norm = :husband_norm AND x.wife_iof = :wife_iof)
   AND id = (SELECT min(y.id) FROM spouse_index y
              WHERE y.husband_norm = :husband_norm
                AND substr(:wife_iof, 1, length(y.wife_iof) + 1) = y.wife_iof || ' '
                AND instr(y.wife_iof, ' ') > 0
                AND coalesce(y.wife_place, '') IN ('', coalesce(:wife_place, '')));

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
-- С учётом НП мужа (Роман 09.10.2026: «к отцу может подтянуться жена его
-- полного тёзки из другой деревни»). НП мужа в памяти пар нет, но есть НП
-- жены, а он тот же: мать получает НП отца, жене после венчания записан НП
-- жениха. Жена из того же пункта — первой; жена без пункта годится; жена
-- из другого пункта — нет. НП мужа неизвестен, а жёны у этого ИОФ из разных
-- пунктов — не угадываем, не подставляем никого.
SELECT wife_iof, wife_place, wife_rank, uses
  FROM spouse_index
 WHERE husband_norm = :husband_norm
   AND (CASE WHEN :place IS NULL
             THEN (SELECT count(DISTINCT nullif(coalesce(x.wife_place, ''), '')) FROM spouse_index x
                    WHERE x.husband_norm = :husband_norm) <= 1
             ELSE coalesce(wife_place, '') IN ('', :place) END)
 ORDER BY (coalesce(wife_place, '') = coalesce(:place, '')) DESC, uses DESC
 LIMIT 1;

-- @wife_husband_rank
-- Звание мужа персоны, запомненной женой («законная жена его»), — чтобы в
-- подсказке показать её звание по мужу (Роман 06.10.2026: «крестьянская
-- жена… мещанская и солдатская»). Муж — из памяти «муж — жена», его звание —
-- последнее набранное.
-- У жён-тёзок первым идёт муж той, чей НП совпал с НП строки подсказки.
SELECT p.rank
  FROM spouse_index s JOIN person_index p ON p.iof_norm = s.husband_norm
 WHERE s.wife_iof = :iof AND trim(coalesce(p.rank, '')) <> ''
 ORDER BY (coalesce(s.wife_place, '') IS NOT coalesce(:place, '')), s.uses DESC,
          coalesce(p.last_used_at, '') DESC, p.id DESC
 LIMIT 1;

-- ============================================================================
--  ВЫГРУЗКА В FAMILIO И В EXCEL (сборка #39, Роман 30.09.2026)
--
--  Сначала export_prepare собирает временные таблицы (x_*): персоны записи
--  с нормализованными и книжными формами имён, раскладку причта и
--  поручителей, пометки и авторский комментарий. Временные — потому что их
--  нужно посчитать один раз на выгрузку: примечания режутся на части
--  рекурсией, и считать её для каждой строки листа было бы долго. Таблицы
--  живут до следующей выгрузки и в базу пользователя не попадают.
--
--  Дальше блоки familio_* и excel_* отдают строки листов: колонки строго в
--  порядке колонок листа образца. Разбор эталона — spec/2026-09-30-sborka-39.md.
-- ============================================================================

-- @export_prepare
DROP TABLE IF EXISTS temp.x_part;
DROP TABLE IF EXISTS temp.x_person0;
DROP TABLE IF EXISTS temp.x_person;
DROP TABLE IF EXISTS temp.x_entry;
DROP TABLE IF EXISTS temp.x_clergy;
DROP TABLE IF EXISTS temp.x_witness;
DROP TABLE IF EXISTS temp.x_text;
DROP TABLE IF EXISTS temp.x_row;
DROP TABLE IF EXISTS temp.x_place;
DROP TABLE IF EXISTS temp.x_village;

-- Пункт → самая полная строка с тем же названием. У Романа 01.10.2026 в
-- выгрузке были пусты полные места: записи ссылались на строку пункта без
-- подробностей, хотя рядом могла лежать полная («двойник»: набран руками
-- или пришёл архивом, а пункт поставки встал рядом). Выгрузка берёт
-- подробности у лучшей строки и показывает пункт на листе location один раз.
CREATE TEMP TABLE x_place AS
SELECT p.id,
       (SELECT b.id FROM place b WHERE b.name_norm = p.name_norm
         ORDER BY (b.full_location IS NULL OR trim(b.full_location) = ''),
                  (b.np_type IS NULL OR trim(b.np_type) = ''), (b.origin = 'archive'), b.id LIMIT 1) AS best_id
  FROM place p;
CREATE INDEX ix_x_place ON x_place (id);

-- Примечания по частям («Имя в документе: Пискарь; того же дому»). У ребёнка
-- примечание — в записи (entry.note): туда его пишет форма рождений.
CREATE TEMP TABLE x_part AS
WITH RECURSIVE src(mention_id, note) AS (
    SELECT m.id, CASE WHEN m.role_code = 'child' THEN e.note ELSE m.note END
      FROM person_mention m JOIN entry e ON e.id = m.entry_id
),
s(mention_id, idx, part, rest) AS (
    SELECT mention_id, 0, NULL, note || ';' FROM src WHERE trim(coalesce(note, '')) <> ''
    UNION ALL
    SELECT mention_id, idx + 1, trim(substr(rest, 1, instr(rest, ';') - 1)), substr(rest, instr(rest, ';') + 1)
      FROM s WHERE rest <> ''
)
SELECT mention_id, idx, part FROM s WHERE idx > 0 AND part <> '';
CREATE INDEX ix_x_part ON x_part (mention_id, idx);

-- Персоны. _b — как набрано; doc_* — как в книге, если при сверке имя
-- заменено словарным («Имя в документе: Пискарь»); _m — современное: имя-основа
-- и отчество «Алексеевич» из словаря (их кладёт разбор ИОФ при сохранении).
CREATE TEMP TABLE x_person0 AS
SELECT m.id, m.entry_id, m.role_code, m.sort_order, m.gender,
       nullif(trim(m.rank), '') AS rank, nullif(trim(m.confession), '') AS confession,
       nullif(trim(m.kinship), '') AS kinship,
       m.age_years, m.age_months, m.age_weeks, m.age_days, nullif(trim(m.age_text), '') AS age_text,
       nullif(trim(m.death_cause), '') AS death_cause, nullif(trim(m.marriage_order), '') AS marriage_order,
       nullif(trim(m.maiden_surname), '') AS maiden_f,
       nullif(trim(m.first_name), '') AS first_b,
       nullif(trim(m.patronymic), '') AS patr_b,
       nullif(trim(m.surname), '') AS surname_b,
       -- «***» — имя в книге не указано (системное слово, 05.10.2026): в
       -- основных полях Familio имени нет.
       nullif(coalesce(nullif(trim(m.first_name_modern), ''), nullif(trim(m.first_name), '')), '***') AS first_m,
       coalesce(nullif(trim(m.patronymic_modern), ''), nullif(trim(m.patronymic), '')) AS patr_m,
       (SELECT nullif(trim(substr(x.part, length('Имя в документе:') + 1)), '') FROM x_part x
         WHERE x.mention_id = m.id AND x.part LIKE 'Имя в документе:%' LIMIT 1) AS doc_first,
       (SELECT nullif(trim(substr(x.part, length('Отчество в документе:') + 1)), '') FROM x_part x
         WHERE x.mention_id = m.id AND x.part LIKE 'Отчество в документе:%' LIMIT 1) AS doc_patr,
       -- Сторона поручителя — первая часть примечания (MarriageForm).
       CASE WHEN m.role_code LIKE 'witness%' THEN
            (SELECT x.part FROM x_part x WHERE x.mention_id = m.id AND x.idx = 1
                AND x.part IN ('по жениху', 'по невесте')) END AS side,
       -- Примечание без пометок сверки (они уходят в авторский комментарий)
       -- и без стороны поручителя (она задаёт колонку).
       (SELECT group_concat(part, '; ') FROM (
            SELECT x.part FROM x_part x WHERE x.mention_id = m.id
               AND x.part NOT LIKE 'Имя в документе:%' AND x.part NOT LIKE 'Отчество в документе:%'
               AND NOT (m.role_code LIKE 'witness%' AND x.idx = 1 AND x.part IN ('по жениху', 'по невесте'))
             ORDER BY x.idx)) AS note_clean,
       -- Полное место; у пункта без подробностей — хотя бы название (Роман
       -- 01.10.2026: person_location заполнялся «лишь частично»).
       -- Название — чистое: комментарий деревни-тёзки («Хмельничное (Столпино)»)
       -- в выгрузки не идёт (Роман 06.10.2026).
       CASE WHEN trim(coalesce(p.comment, '')) <> ''
                 AND substr(p.name, -length(trim(p.comment)) - 3) = ' (' || trim(p.comment) || ')'
            THEN substr(p.name, 1, length(p.name) - length(trim(p.comment)) - 3) ELSE p.name END AS place,
       coalesce(nullif(trim(p.full_location), ''), nullif(trim(p.short_location), ''),
                CASE WHEN trim(coalesce(p.comment, '')) <> ''
                 AND substr(p.name, -length(trim(p.comment)) - 3) = ' (' || trim(p.comment) || ')'
            THEN substr(p.name, 1, length(p.name) - length(trim(p.comment)) - 3) ELSE p.name END) AS place_full,
       -- Причт — по званию, а не по номеру: у Романа во втором причте 733
       -- псаломщика, и по номеру они легли бы в колонки дьякона (разбор эталона).
       -- Сравнение без первой буквы — LIKE не знает регистра кириллицы.
       CASE WHEN m.role_code NOT LIKE 'clergy%' THEN NULL
            WHEN m.rank LIKE '%вящен%' OR m.rank LIKE '%ерей%' OR m.rank LIKE '%ротопоп%'
              OR m.rank LIKE '%гумен%' OR m.rank LIKE '%еромонах%' OR m.rank LIKE '%рхимандрит%' THEN 1
            WHEN m.rank LIKE '%иакон%' OR m.rank LIKE '%ьякон%' THEN 2
            ELSE 3 END AS clergy_cat
  FROM person_mention m
  LEFT JOIN x_place xp ON xp.id = m.place_id
  LEFT JOIN place p ON p.id = xp.best_id;

CREATE TEMP TABLE x_person AS
SELECT q.*,
       trim(coalesce(q.first_b || ' ', '') || coalesce(q.patr_b || ' ', '') || coalesce(q.surname_b, '')) AS iof_b,
       -- Как в книге: «Никита Алексеев».
       trim(coalesce(coalesce(q.doc_first, q.first_b) || ' ', '') || coalesce(coalesce(q.doc_patr, q.patr_b) || ' ', '')
            || coalesce(q.surname_b, '')) AS iof_doc,
       -- Как в книге, но фамилией вперёд, если есть и отчество (так причт и
       -- метки пометок у индексатора): «Промтов Василий Васильев».
       CASE WHEN q.patr_b IS NOT NULL AND q.surname_b IS NOT NULL
            THEN q.surname_b || ' ' || coalesce(q.doc_first, q.first_b, '') || ' ' || coalesce(q.doc_patr, q.patr_b)
            ELSE trim(coalesce(coalesce(q.doc_first, q.first_b) || ' ', '') || coalesce(coalesce(q.doc_patr, q.patr_b) || ' ', '')
                      || coalesce(q.surname_b, '')) END AS fio_book,
       -- Фамилия без девичьей в скобках: «Иванова (Петрова)» → «Иванова».
       nullif(trim(CASE WHEN instr(q.surname_b, '(') > 0 THEN substr(q.surname_b, 1, instr(q.surname_b, '(') - 1)
                        ELSE q.surname_b END), '') AS surname_base,
       coalesce(q.maiden_f, nullif(trim(replace(CASE WHEN instr(q.surname_b, '(') > 0
                                                     THEN substr(q.surname_b, instr(q.surname_b, '(') + 1) END, ')', '')), '')) AS maiden
  FROM x_person0 q;
CREATE INDEX ix_x_person ON x_person (entry_id, role_code);

-- Село дела → его пункт и чистое название. В деле село может стоять и чистым
-- («Никольское»), и с комментарием деревни-тёзки («Никольское (Галичский
-- уезд)» — выбрано из подсказки), а пункт называется с комментарием: без
-- этой таблицы место события теряло бы полное место, а комментарий уходил
-- бы в выгрузку и в «Село:» листа Excel (ревьюер 08.10.2026). Сначала точное
-- совпадение названия, потом — чистого названия.
DROP TABLE IF EXISTS temp.x_village;
CREATE TEMP TABLE x_village AS
SELECT raw, clean, best_id
  FROM (SELECT v.raw AS raw, coalesce(pc.clean, v.raw) AS clean, xp.best_id AS best_id,
               row_number() OVER (PARTITION BY v.raw
                                  ORDER BY (pc.id IS NULL), (pc.name IS NOT v.raw), pc.id) AS rn
          FROM (SELECT DISTINCT trim(village) AS raw FROM mk_case WHERE trim(coalesce(village, '')) <> '') v
          LEFT JOIN (SELECT p.id, p.name,
                            CASE WHEN trim(coalesce(p.comment, '')) <> ''
                         AND substr(p.name, -length(trim(p.comment)) - 3) = ' (' || trim(p.comment) || ')'
                    THEN substr(p.name, 1, length(p.name) - length(trim(p.comment)) - 3) ELSE p.name END AS clean
                       FROM place p) pc ON pc.name = v.raw OR pc.clean = v.raw
          LEFT JOIN x_place xp ON xp.id = pc.id)
 WHERE rn = 1;

-- Записи с делом и местом события. НП события — село дела, его карточка —
-- по названию (как place_get: не из архива, самая ранняя).
CREATE TEMP TABLE x_entry AS
SELECT e.id, e.section, e.page, e.no_male, e.no_female,
       e.event_day, e.event_month, e.event_year, e.rite_day, e.rite_month, e.rite_year,
       coalesce(e.rite_year, e.event_year) AS book_year,
       coalesce(e.updated_at, e.created_at) AS changed_at,
       nullif(trim(c.archive), '') AS archive, nullif(trim(c.fond), '') AS fond,
       nullif(trim(c.opis), '') AS opis, nullif(trim(c.delo), '') AS delo,
       nullif(trim(c.church), '') AS church,
       (SELECT xv.clean FROM x_village xv WHERE xv.raw = trim(c.village)) AS village,
       (SELECT xv.best_id FROM x_village xv WHERE xv.raw = trim(c.village)) AS village_place,
       nullif(trim(c.uyezd), '') AS uyezd, nullif(trim(c.guberniya), '') AS guberniya,
       -- Самая полная строка пункта с этим названием; нет карточки или
       -- подробностей — название (Роман 01.10.2026: full_location был пуст).
       coalesce((SELECT nullif(trim(b.full_location), '') FROM x_village xv JOIN place b ON b.id = xv.best_id
                  WHERE xv.raw = trim(c.village)),
                (SELECT xv.clean FROM x_village xv WHERE xv.raw = trim(c.village))) AS village_full
  FROM entry e JOIN mk_case c ON c.id = e.case_id;

-- Причт по колонкам: 1 — священник, 2 — дьякон, 3 — псаломщик (и дьячок,
-- пономарь). Второй с тем же званием — в свободную колонку.
CREATE TEMP TABLE x_clergy AS
WITH c AS (
    SELECT entry_id, id, sort_order, clergy_cat AS cat, rank, fio_book AS fio, note_clean,
           row_number() OVER (PARTITION BY entry_id, clergy_cat ORDER BY sort_order, id) AS rn
      FROM x_person WHERE role_code LIKE 'clergy%' AND iof_b <> ''
),
firsts AS (SELECT entry_id, cat AS slot FROM c WHERE rn = 1),
extra AS (SELECT c.*, row_number() OVER (PARTITION BY entry_id ORDER BY sort_order, id) AS xn FROM c WHERE rn > 1),
free AS (
    SELECT e.entry_id, s.slot, row_number() OVER (PARTITION BY e.entry_id ORDER BY s.slot) AS fn
      FROM (SELECT DISTINCT entry_id FROM extra) e
     CROSS JOIN (SELECT 1 AS slot UNION ALL SELECT 2 UNION ALL SELECT 3) s
     WHERE NOT EXISTS (SELECT 1 FROM firsts f WHERE f.entry_id = e.entry_id AND f.slot = s.slot)
)
SELECT entry_id, cat AS slot, rank, fio, note_clean FROM c WHERE rn = 1
UNION ALL
SELECT x.entry_id, f.slot, x.rank, x.fio, x.note_clean
  FROM extra x JOIN free f ON f.entry_id = x.entry_id AND f.fn = x.xn;
CREATE INDEX ix_x_clergy ON x_clergy (entry_id, slot);

-- Поручители по сторонам: блоки 1–3 — по жениху, 4–6 — по невесте (как в
-- образце Familio). У индексатора — по номеру, и поручитель по невесте
-- попадал под «№3 по жениху» (разбор эталона). Четвёртый с одной стороны —
-- в свободный блок другой.
CREATE TEMP TABLE x_witness AS
WITH w AS (
    SELECT entry_id, id, sort_order, CASE WHEN side = 'по невесте' THEN 2 ELSE 1 END AS s,
           row_number() OVER (PARTITION BY entry_id, CASE WHEN side = 'по невесте' THEN 2 ELSE 1 END
                              ORDER BY sort_order, id) AS rn
      FROM x_person WHERE role_code LIKE 'witness%' AND iof_b <> ''
),
placed AS (SELECT entry_id, id, (s - 1) * 3 + rn AS block FROM w WHERE rn <= 3),
extra AS (SELECT entry_id, id, row_number() OVER (PARTITION BY entry_id ORDER BY sort_order, id) AS xn FROM w WHERE rn > 3),
free AS (
    SELECT e.entry_id, b.block, row_number() OVER (PARTITION BY e.entry_id ORDER BY b.block) AS fn
      FROM (SELECT DISTINCT entry_id FROM extra) e
     CROSS JOIN (SELECT 1 AS block UNION ALL SELECT 2 UNION ALL SELECT 3
                 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6) b
     WHERE NOT EXISTS (SELECT 1 FROM placed p WHERE p.entry_id = e.entry_id AND p.block = b.block)
)
SELECT entry_id, id, block FROM placed
UNION ALL
SELECT x.entry_id, x.id, f.block FROM extra x JOIN free f ON f.entry_id = x.entry_id AND f.fn = x.xn;
CREATE INDEX ix_x_witness ON x_witness (entry_id, block);

-- Отчество ребёнка по имени отца (лист «МК» и авторский комментарий): поиск по name_dict.name шёл
-- полным проходом словаря на каждого ребёнка — 5 с на 20 000 записей
-- (проверяющий #39). Своя маленькая таблица с индексом.
DROP TABLE IF EXISTS temp.x_patr;
CREATE TEMP TABLE x_patr AS
SELECT name, min(patr_m) AS patr_m, min(patr_f) AS patr_f
  FROM name_dict WHERE coalesce(base_name, '') = '' GROUP BY name;
CREATE INDEX ix_x_patr ON x_patr (name);

-- Пометки и авторский комментарий записи.
-- Комментарий — как в индексаторе (Роман 01.10.2026: «Делаем как в
-- индексаторе»): «Имена в МК: ИОФ; ИОФ…» — написание из книги тех персон, у
-- кого оно отличается от нормализованного «Имя Отчество Фамилия»; причт не
-- входит; отличий нет — пусто. У ребёнка нормализованное — с отчеством по
-- имени отца и отцовской фамилией, поэтому он попадает почти всегда.
-- Порядок его: в браке родственник жениха — сразу за женихом.
-- Пометки — «ИОФ: примечание; …», как у индексатора, но без полей, у
-- которых в образце своя колонка (каким браком, сторона, причина смерти).
CREATE TEMP TABLE x_text AS
SELECT e.id AS entry_id,
       (SELECT 'Имена в МК: ' || group_concat(iof_doc, '; ') FROM (
            SELECT p.iof_doc
              FROM x_person p
              LEFT JOIN x_person f ON p.role_code = 'child' AND f.entry_id = p.entry_id AND f.role_code = 'father'
              LEFT JOIN x_patr d ON d.name = f.first_m
             WHERE p.entry_id = e.id AND p.iof_doc <> '' AND p.role_code NOT LIKE 'clergy%'
               AND p.iof_doc <> trim(coalesce(p.first_m || ' ', '')
                     || coalesce(CASE WHEN p.role_code = 'child'
                                      THEN CASE WHEN p.gender = 'Ж' THEN d.patr_f ELSE d.patr_m END
                                      ELSE p.patr_m END || ' ', '')
                     || coalesce(CASE WHEN p.role_code = 'child' THEN f.surname_base ELSE p.surname_b END, ''))
             ORDER BY CASE p.role_code WHEN 'groom' THEN 1 WHEN 'groom_relative' THEN 2 WHEN 'bride' THEN 3
                                       WHEN 'bride_relative' THEN 4 WHEN 'bride_parent' THEN 4
                                       ELSE 10 + p.sort_order END, p.id)) AS author,
       (SELECT group_concat(lbl || ': ' || note_clean, '; ') FROM (
            SELECT coalesce(nullif(p.fio_book, ''), r.title) AS lbl, p.note_clean
              FROM x_person p LEFT JOIN role r ON r.code = p.role_code
             WHERE p.entry_id = e.id AND p.note_clean IS NOT NULL
             ORDER BY p.sort_order, p.id)) AS notes
  FROM entry e;
CREATE INDEX ix_x_text ON x_text (entry_id);


-- Строки листов «1», «2», «3» индексатора (выгрузка в Excel и лист «МК»):
-- 55 колонок A…BC как у него, у браков ещё 8 — поручители 5 и 6, которых
-- у индексатора нет (Роман 27.09.2026 просил до шести).
-- ИОФ — как в книге, даты — текстом «05.01.1886».
CREATE TEMP TABLE x_row AS
WITH d AS (
    SELECT e.*,
           row_number() OVER (PARTITION BY e.section ORDER BY e.book_year, e.id) AS n,
           nullif(trim(coalesce('Ф.' || e.fond, '') || coalesce(' Оп.' || e.opis, '') || coalesce(' Д.' || e.delo, '')), '') AS fod,
           'Церковь: ' || coalesce(e.church, '') || ' Село: ' || coalesce(e.village, '')
             || ' Уезд: ' || coalesce(e.uyezd, '') || ' Губерния: ' || coalesce(e.guberniya, '') AS mk,
           CASE WHEN e.event_day IS NULL AND e.event_month IS NULL AND e.event_year IS NULL THEN NULL
                ELSE CASE WHEN e.event_day IS NULL THEN '??' ELSE printf('%02d', e.event_day) END || '.'
                  || CASE WHEN e.event_month IS NULL THEN '??' ELSE printf('%02d', e.event_month) END || '.'
                  || coalesce(e.event_year, '????') END AS event_date,
           CASE WHEN e.rite_day IS NULL AND e.rite_month IS NULL AND e.rite_year IS NULL THEN NULL
                ELSE CASE WHEN e.rite_day IS NULL THEN '??' ELSE printf('%02d', e.rite_day) END || '.'
                  || CASE WHEN e.rite_month IS NULL THEN '??' ELSE printf('%02d', e.rite_month) END || '.'
                  || coalesce(e.rite_year, '????') END AS rite_date
      FROM x_entry e
)
SELECT d.id AS entry_id, d.section,
       d.n AS c1, d.archive AS c2, d.fod AS c3, d.mk AS c4, d.book_year AS c5, 1 AS c6, d.page AS c7,
       d.no_male AS c8, d.no_female AS c9, d.event_date AS c10, d.rite_date AS c11,
       ch.iof_doc AS c12,
       f.place AS c13, f.rank AS c14, nullif(f.iof_doc, '') AS c15, f.confession AS c16, f.note_clean AS c17, NULL AS c18,
       m.place AS c19, m.rank AS c20, nullif(m.iof_doc, '') AS c21, m.confession AS c22, m.note_clean AS c23, NULL AS c24,
       NULL AS c25, NULL AS c26, NULL AS c27, NULL AS c28, NULL AS c29, NULL AS c30,
       nullif(g1.iof_doc, '') AS c31, g1.place AS c32, g1.rank AS c33, g1.note_clean AS c34,
       nullif(g2.iof_doc, '') AS c35, g2.place AS c36, g2.rank AS c37, g2.note_clean AS c38,
       nullif(g3.iof_doc, '') AS c39, g3.place AS c40, g3.rank AS c41, g3.note_clean AS c42,
       nullif(g4.iof_doc, '') AS c43, g4.place AS c44, g4.rank AS c45, g4.note_clean AS c46,
       nullif(k1.iof_doc, '') AS c47, k1.rank AS c48, k1.note_clean AS c49,
       nullif(k2.iof_doc, '') AS c50, k2.rank AS c51, k2.note_clean AS c52,
       nullif(k3.iof_doc, '') AS c53, k3.rank AS c54, k3.note_clean AS c55,
       NULL AS c56, NULL AS c57, NULL AS c58, NULL AS c59, NULL AS c60, NULL AS c61, NULL AS c62, NULL AS c63
  FROM d
  LEFT JOIN x_person ch ON ch.entry_id = d.id AND ch.role_code = 'child'
  LEFT JOIN x_person f ON f.entry_id = d.id AND f.role_code = 'father'
  LEFT JOIN x_person m ON m.entry_id = d.id AND m.role_code = 'mother'
  LEFT JOIN x_person g1 ON g1.entry_id = d.id AND g1.role_code = 'godparent1'
  LEFT JOIN x_person g2 ON g2.entry_id = d.id AND g2.role_code = 'godparent2'
  LEFT JOIN x_person g3 ON g3.entry_id = d.id AND g3.role_code = 'godparent3'
  LEFT JOIN x_person g4 ON g4.entry_id = d.id AND g4.role_code = 'godparent4'
  LEFT JOIN x_person k1 ON k1.entry_id = d.id AND k1.role_code = 'clergy1'
  LEFT JOIN x_person k2 ON k2.entry_id = d.id AND k2.role_code = 'clergy2'
  LEFT JOIN x_person k3 ON k3.entry_id = d.id AND k3.role_code = 'clergy3'
 WHERE d.section = 1
UNION ALL
SELECT d.id, d.section,
       d.n, d.archive, d.fod, d.mk, d.book_year, 2, d.page,
       d.no_male, NULL, d.event_date, NULL,
       NULL,
       g.place, g.rank, nullif(g.iof_doc, ''), g.confession,
       -- «Прим.» жениха у индексатора — «каким браком»; своё примечание — следом.
       nullif(trim(coalesce(g.marriage_order, '') || coalesce('; ' || g.note_clean, ''), '; '), ''), g.age_years,
       b.place, b.rank, nullif(b.iof_doc, ''), b.confession,
       nullif(trim(coalesce(b.marriage_order, '') || coalesce('; ' || b.note_clean, ''), '; '), ''), b.age_years,
       gr.kinship, nullif(gr.iof_doc, ''), gr.rank, br.kinship, nullif(br.iof_doc, ''), br.rank,
       nullif(w1.iof_doc, ''), w1.place, w1.rank, nullif(trim(coalesce(w1.side, '') || coalesce('; ' || w1.note_clean, ''), '; '), ''),
       nullif(w2.iof_doc, ''), w2.place, w2.rank, nullif(trim(coalesce(w2.side, '') || coalesce('; ' || w2.note_clean, ''), '; '), ''),
       nullif(w3.iof_doc, ''), w3.place, w3.rank, nullif(trim(coalesce(w3.side, '') || coalesce('; ' || w3.note_clean, ''), '; '), ''),
       nullif(w4.iof_doc, ''), w4.place, w4.rank, nullif(trim(coalesce(w4.side, '') || coalesce('; ' || w4.note_clean, ''), '; '), ''),
       nullif(k1.iof_doc, ''), k1.rank, k1.note_clean,
       nullif(k2.iof_doc, ''), k2.rank, k2.note_clean,
       nullif(k3.iof_doc, ''), k3.rank, k3.note_clean,
       nullif(w5.iof_doc, ''), w5.place, w5.rank, nullif(trim(coalesce(w5.side, '') || coalesce('; ' || w5.note_clean, ''), '; '), ''),
       nullif(w6.iof_doc, ''), w6.place, w6.rank, nullif(trim(coalesce(w6.side, '') || coalesce('; ' || w6.note_clean, ''), '; '), '')
  FROM d
  LEFT JOIN x_person g ON g.entry_id = d.id AND g.role_code = 'groom'
  LEFT JOIN x_person b ON b.entry_id = d.id AND b.role_code = 'bride'
  LEFT JOIN x_person gr ON gr.entry_id = d.id AND gr.role_code = 'groom_relative'
  LEFT JOIN x_person br ON br.entry_id = d.id AND br.role_code IN ('bride_relative', 'bride_parent')
  LEFT JOIN x_person w1 ON w1.entry_id = d.id AND w1.role_code = 'witness1'
  LEFT JOIN x_person w2 ON w2.entry_id = d.id AND w2.role_code = 'witness2'
  LEFT JOIN x_person w3 ON w3.entry_id = d.id AND w3.role_code = 'witness3'
  LEFT JOIN x_person w4 ON w4.entry_id = d.id AND w4.role_code = 'witness4'
  LEFT JOIN x_person w5 ON w5.entry_id = d.id AND w5.role_code = 'witness5'
  LEFT JOIN x_person w6 ON w6.entry_id = d.id AND w6.role_code = 'witness6'
  LEFT JOIN x_person k1 ON k1.entry_id = d.id AND k1.role_code = 'clergy1'
  LEFT JOIN x_person k2 ON k2.entry_id = d.id AND k2.role_code = 'clergy2'
  LEFT JOIN x_person k3 ON k3.entry_id = d.id AND k3.role_code = 'clergy3'
 WHERE d.section = 2
UNION ALL
SELECT d.id, d.section,
       d.n, d.archive, d.fod, d.mk, d.book_year, 3, d.page,
       d.no_male, d.no_female, d.event_date, d.rite_date,
       NULL,
       u.place, u.rank, nullif(u.iof_doc, ''), u.confession,
       -- «Прим.» умершего у индексатора — «от чего умер».
       nullif(trim(coalesce(u.death_cause, '') || coalesce('; ' || u.note_clean, ''), '; '), ''),
       coalesce(u.age_text, u.age_years),
       -- «Прим.» родственника в строке 2 шапки — «отец, супруг»: родство.
       r.place, r.rank, nullif(r.iof_doc, ''), r.confession,
       nullif(trim(coalesce(r.kinship, '') || coalesce('; ' || r.note_clean, ''), '; '), ''), NULL,
       NULL, NULL, NULL, NULL, NULL, NULL,
       NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL,
       nullif(k1.iof_doc, ''), k1.rank, k1.note_clean,
       nullif(k2.iof_doc, ''), k2.rank, k2.note_clean,
       nullif(k3.iof_doc, ''), k3.rank, k3.note_clean,
       NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL
  FROM d
  LEFT JOIN x_person u ON u.entry_id = d.id AND u.role_code = 'deceased'
  LEFT JOIN x_person r ON r.entry_id = d.id AND r.role_code IN ('deceased_relative', 'deceased_parent', 'deceased_spouse')
  LEFT JOIN x_person k1 ON k1.entry_id = d.id AND k1.role_code = 'clergy1'
  LEFT JOIN x_person k2 ON k2.entry_id = d.id AND k2.role_code = 'clergy2'
  LEFT JOIN x_person k3 ON k3.entry_id = d.id AND k3.role_code = 'clergy3'
 WHERE d.section = 3;
CREATE INDEX ix_x_row ON x_row (entry_id);

-- @export_years
-- Годы книги, по которым есть записи, — для окна выгрузки в Familio.
-- Год книги — год обряда (крещения, погребения), у браков — год венчания.
-- Строка с year = NULL — записи без года (до 22.09 год не был обязателен):
-- они выгружаются только со всем приходом (:years = '[]'), окно об этом
-- говорит (проверяющий #39: иначе пропадали молча).
SELECT coalesce(rite_year, event_year) AS year,
       sum(section = 1) AS births, sum(section = 2) AS marriages, sum(section = 3) AS deaths
  FROM entry
 GROUP BY 1 ORDER BY year IS NULL, 1;

-- @familio_birth
-- Лист РОЖДЕНИЕ образца Familio: 69 колонок A…BQ. :years — годы книги
-- JSON-массивом, «[]» — весь приход. Отчества — современные (Роман
-- 30.09.2026), Н.П. — название без типа, как в колонке A листа location:
-- по нему образец ищет полное место (ВПР). День и месяц — числами, как в
-- образце (у индексатора — текстом «05»).
SELECT row_number() OVER (ORDER BY e.book_year, e.id),
       e.archive, e.fond, e.opis, e.delo, e.page,
       e.village, e.village_full, e.church,
       CASE WHEN e.rite_year > e.event_year + 3 THEN 'присоединение' ELSE 'рождение' END,
       coalesce(e.no_male, e.no_female),
       CASE coalesce(ch.gender, CASE WHEN e.no_female IS NOT NULL THEN 'Ж' WHEN e.no_male IS NOT NULL THEN 'М' END)
            WHEN 'Ж' THEN 'жен' WHEN 'М' THEN 'муж' END,
       e.event_day, e.event_month, e.event_year, e.rite_day, e.rite_month, e.rite_year,
       ch.first_m,
       f.place, f.place_full, f.rank, f.surname_base, NULL, f.first_m, f.patr_m,
       m.rank, m.surname_base, m.maiden, m.first_m, m.patr_m,
       g1.rank, g1.place, g1.place_full, nullif(trim(coalesce(g1.surname_base || ' ', '') || coalesce(g1.first_m || ' ', '') || coalesce(g1.patr_m, '')), ''),
       g2.rank, g2.place, g2.place_full, nullif(trim(coalesce(g2.surname_base || ' ', '') || coalesce(g2.first_m || ' ', '') || coalesce(g2.patr_m, '')), ''),
       g3.rank, g3.place, g3.place_full, nullif(trim(coalesce(g3.surname_base || ' ', '') || coalesce(g3.first_m || ' ', '') || coalesce(g3.patr_m, '')), ''),
       g4.rank, g4.place, g4.place_full, nullif(trim(coalesce(g4.surname_base || ' ', '') || coalesce(g4.first_m || ' ', '') || coalesce(g4.patr_m, '')), ''),
       NULL, NULL, NULL, NULL, NULL, NULL,
       NULL, NULL, NULL, NULL, NULL, NULL,
       k1.rank, k1.fio, NULL, NULL, k2.rank, k2.fio, k3.rank, k3.fio,
       t.notes, t.author
  FROM x_entry e
  LEFT JOIN x_person ch ON ch.entry_id = e.id AND ch.role_code = 'child'
  LEFT JOIN x_person f ON f.entry_id = e.id AND f.role_code = 'father'
  LEFT JOIN x_person m ON m.entry_id = e.id AND m.role_code = 'mother'
  LEFT JOIN x_person g1 ON g1.entry_id = e.id AND g1.role_code = 'godparent1'
  LEFT JOIN x_person g2 ON g2.entry_id = e.id AND g2.role_code = 'godparent2'
  LEFT JOIN x_person g3 ON g3.entry_id = e.id AND g3.role_code = 'godparent3'
  LEFT JOIN x_person g4 ON g4.entry_id = e.id AND g4.role_code = 'godparent4'
  LEFT JOIN x_clergy k1 ON k1.entry_id = e.id AND k1.slot = 1
  LEFT JOIN x_clergy k2 ON k2.entry_id = e.id AND k2.slot = 2
  LEFT JOIN x_clergy k3 ON k3.entry_id = e.id AND k3.slot = 3
  LEFT JOIN x_text t ON t.entry_id = e.id
 WHERE e.section = 1
   AND (:years = '[]' OR e.book_year IN (SELECT value FROM json_each(:years)))
 ORDER BY e.book_year, e.id;

-- @familio_marriage
-- Лист БРАК: 97 колонок A…CS. «Отец жениха» и «Отец невесты» — родственник
-- с родством «отец» (или без родства) со своим званием; другой родственник —
-- в блок «Родственник … жениха/невесты» с родством. Поручители — по сторонам
-- (x_witness): блоки 1–3 по жениху, 4–6 по невесте.
SELECT row_number() OVER (ORDER BY e.book_year, e.id),
       e.archive, e.fond, e.opis, e.delo, e.page,
       e.village, e.village_full, e.church, 'брак', e.no_male,
       e.event_day, e.event_month, e.event_year,
       g.place, g.place_full, g.rank, g.surname_base, NULL, g.first_m, g.patr_m,
       CASE WHEN g.marriage_order LIKE 'перв%' OR g.marriage_order LIKE 'Перв%' THEN 1
            WHEN g.marriage_order LIKE 'втор%' OR g.marriage_order LIKE 'Втор%' THEN 2
            WHEN g.marriage_order LIKE 'трет%' OR g.marriage_order LIKE 'Трет%' THEN 3
            WHEN g.marriage_order LIKE 'четв%' OR g.marriage_order LIKE 'Четв%' THEN 4
            ELSE g.marriage_order END,
       g.age_years, NULL, NULL, NULL,
       gf.place, gf.place_full, gf.rank, gf.surname_base, NULL, gf.first_m, gf.patr_m,
       b.place, b.place_full, b.rank, b.surname_b, b.first_m, b.patr_m,
       CASE WHEN b.marriage_order LIKE 'перв%' OR b.marriage_order LIKE 'Перв%' THEN 1
            WHEN b.marriage_order LIKE 'втор%' OR b.marriage_order LIKE 'Втор%' THEN 2
            WHEN b.marriage_order LIKE 'трет%' OR b.marriage_order LIKE 'Трет%' THEN 3
            WHEN b.marriage_order LIKE 'четв%' OR b.marriage_order LIKE 'Четв%' THEN 4
            ELSE b.marriage_order END,
       b.age_years, NULL, NULL, NULL,
       bf.place, bf.place_full, bf.rank, bf.surname_base, NULL, bf.first_m, bf.patr_m,
       w1.rank, w1.place, w1.place_full, nullif(trim(coalesce(w1.surname_base || ' ', '') || coalesce(w1.first_m || ' ', '') || coalesce(w1.patr_m, '')), ''),
       w2.rank, w2.place, w2.place_full, nullif(trim(coalesce(w2.surname_base || ' ', '') || coalesce(w2.first_m || ' ', '') || coalesce(w2.patr_m, '')), ''),
       w3.rank, w3.place, w3.place_full, nullif(trim(coalesce(w3.surname_base || ' ', '') || coalesce(w3.first_m || ' ', '') || coalesce(w3.patr_m, '')), ''),
       w4.rank, w4.place, w4.place_full, nullif(trim(coalesce(w4.surname_base || ' ', '') || coalesce(w4.first_m || ' ', '') || coalesce(w4.patr_m, '')), ''),
       w5.rank, w5.place, w5.place_full, nullif(trim(coalesce(w5.surname_base || ' ', '') || coalesce(w5.first_m || ' ', '') || coalesce(w5.patr_m, '')), ''),
       w6.rank, w6.place, w6.place_full, nullif(trim(coalesce(w6.surname_base || ' ', '') || coalesce(w6.first_m || ' ', '') || coalesce(w6.patr_m, '')), ''),
       CASE WHEN gx.id IS NOT NULL THEN 'жениха' END, gx.kinship, gx.rank, gx.place, gx.place_full,
       nullif(trim(coalesce(gx.surname_base || ' ', '') || coalesce(gx.first_m || ' ', '') || coalesce(gx.patr_m, '')), ''),
       CASE WHEN bx.id IS NOT NULL THEN 'невесты' END, bx.kinship, bx.rank, bx.place, bx.place_full,
       nullif(trim(coalesce(bx.surname_base || ' ', '') || coalesce(bx.first_m || ' ', '') || coalesce(bx.patr_m, '')), ''),
       k1.rank, k1.fio, NULL, NULL, k2.rank, k2.fio, k3.rank, k3.fio,
       t.notes, t.author
  FROM x_entry e
  LEFT JOIN x_person g ON g.entry_id = e.id AND g.role_code = 'groom'
  LEFT JOIN x_person b ON b.entry_id = e.id AND b.role_code = 'bride'
  LEFT JOIN x_person gf ON gf.entry_id = e.id AND gf.role_code = 'groom_relative'
        AND coalesce(gf.kinship, 'отец') = 'отец' AND gf.iof_b <> ''
  LEFT JOIN x_person gx ON gx.entry_id = e.id AND gx.role_code = 'groom_relative'
        AND coalesce(gx.kinship, 'отец') <> 'отец' AND gx.iof_b <> ''
  LEFT JOIN x_person bf ON bf.entry_id = e.id AND bf.role_code IN ('bride_relative', 'bride_parent')
        AND coalesce(bf.kinship, 'отец') = 'отец' AND bf.iof_b <> ''
  LEFT JOIN x_person bx ON bx.entry_id = e.id AND bx.role_code IN ('bride_relative', 'bride_parent')
        AND coalesce(bx.kinship, 'отец') <> 'отец' AND bx.iof_b <> ''
  LEFT JOIN x_witness xw1 ON xw1.entry_id = e.id AND xw1.block = 1 LEFT JOIN x_person w1 ON w1.id = xw1.id
  LEFT JOIN x_witness xw2 ON xw2.entry_id = e.id AND xw2.block = 2 LEFT JOIN x_person w2 ON w2.id = xw2.id
  LEFT JOIN x_witness xw3 ON xw3.entry_id = e.id AND xw3.block = 3 LEFT JOIN x_person w3 ON w3.id = xw3.id
  LEFT JOIN x_witness xw4 ON xw4.entry_id = e.id AND xw4.block = 4 LEFT JOIN x_person w4 ON w4.id = xw4.id
  LEFT JOIN x_witness xw5 ON xw5.entry_id = e.id AND xw5.block = 5 LEFT JOIN x_person w5 ON w5.id = xw5.id
  LEFT JOIN x_witness xw6 ON xw6.entry_id = e.id AND xw6.block = 6 LEFT JOIN x_person w6 ON w6.id = xw6.id
  LEFT JOIN x_clergy k1 ON k1.entry_id = e.id AND k1.slot = 1
  LEFT JOIN x_clergy k2 ON k2.entry_id = e.id AND k2.slot = 2
  LEFT JOIN x_clergy k3 ON k3.entry_id = e.id AND k3.slot = 3
  LEFT JOIN x_text t ON t.entry_id = e.id
 WHERE e.section = 2
   AND (:years = '[]' OR e.book_year IN (SELECT value FROM json_each(:years)))
 ORDER BY e.book_year, e.id;

-- @familio_death
-- Лист СМЕРТЬ: 53 колонки A…BA. Родство — из родства родственника (у
-- индексатора сюда попадало его примечание). Погребение совершал — священник;
-- исповедовал — тот из причта, у кого в примечании «исповед». Умерший без
-- имени («личность не установлена») выгружается без ИОФ, со званием.
SELECT row_number() OVER (ORDER BY e.book_year, e.id),
       e.archive, e.fond, e.opis, e.delo, e.page,
       e.village, e.village_full, e.church, 'смерть', coalesce(e.no_male, e.no_female),
       CASE coalesce(u.gender, CASE WHEN e.no_female IS NOT NULL THEN 'Ж' WHEN e.no_male IS NOT NULL THEN 'М' END)
            WHEN 'Ж' THEN 'жен' WHEN 'М' THEN 'муж' END,
       e.event_day, e.event_month, e.event_year, e.rite_day, e.rite_month, e.rite_year,
       u.place, u.place_full, u.rank, u.surname_base, NULL, u.first_m, u.patr_m,
       u.age_years, u.age_months, u.age_weeks, u.age_days,
       r.kinship, r.place, r.place_full, r.rank, r.surname_base, NULL, r.first_m, r.patr_m,
       u.death_cause,
       kc.rank, kc.fio, NULL, NULL,
       k1.rank, k1.fio, NULL, NULL,
       k2.rank, k2.fio, k3.rank, k3.fio,
       NULL,
       t.notes, t.author
  FROM x_entry e
  LEFT JOIN x_person u ON u.entry_id = e.id AND u.role_code = 'deceased'
  LEFT JOIN x_person r ON r.entry_id = e.id AND r.role_code IN ('deceased_relative', 'deceased_parent', 'deceased_spouse')
  LEFT JOIN x_clergy k1 ON k1.entry_id = e.id AND k1.slot = 1
  LEFT JOIN x_clergy k2 ON k2.entry_id = e.id AND k2.slot = 2
  LEFT JOIN x_clergy k3 ON k3.entry_id = e.id AND k3.slot = 3
  LEFT JOIN (SELECT entry_id, min(slot) AS slot FROM x_clergy WHERE note_clean LIKE '%сповед%' GROUP BY entry_id) kcs
         ON kcs.entry_id = e.id
  LEFT JOIN x_clergy kc ON kc.entry_id = e.id AND kc.slot = kcs.slot
  LEFT JOIN x_text t ON t.entry_id = e.id
 WHERE e.section = 3
   AND (:years = '[]' OR e.book_year IN (SELECT value FROM json_each(:years)))
 ORDER BY e.book_year, e.id;

-- @familio_location
-- Лист location: населённые пункты выгружаемых записей — село дела и места
-- всех персон. 9 колонок A…I. Губерния, уезд, волость — со словом, если его
-- нет в значении («Костромская» → «Костромская губерния»), как в НП!G
-- индексатора. I — код пункта из ссылки Familio.
WITH chosen AS (
    SELECT id FROM x_entry
     WHERE :years = '[]' OR book_year IN (SELECT value FROM json_each(:years))
),
used AS (
    SELECT DISTINCT xp.best_id AS id
      FROM person_mention m JOIN chosen c ON c.id = m.entry_id JOIN x_place xp ON xp.id = m.place_id
    UNION
    SELECT e.village_place FROM x_entry e JOIN chosen c ON c.id = e.id
)
SELECT CASE WHEN trim(coalesce(p.comment, '')) <> ''
                 AND substr(p.name, -length(trim(p.comment)) - 3) = ' (' || trim(p.comment) || ')'
            THEN substr(p.name, 1, length(p.name) - length(trim(p.comment)) - 3) ELSE p.name END, p.np_type,
       CASE WHEN p.guberniya IS NULL OR trim(p.guberniya) = '' THEN NULL
            WHEN instr(trim(p.guberniya), ' ') > 0 THEN trim(p.guberniya) ELSE trim(p.guberniya) || ' губерния' END,
       CASE WHEN p.uyezd IS NULL OR trim(p.uyezd) = '' THEN NULL
            WHEN instr(trim(p.uyezd), ' ') > 0 THEN trim(p.uyezd) ELSE trim(p.uyezd) || ' уезд' END,
       CASE WHEN p.volost IS NULL OR trim(p.volost) = '' THEN NULL
            WHEN instr(trim(p.volost), ' ') > 0 THEN trim(p.volost) ELSE trim(p.volost) || ' волость' END,
       coalesce(p.short_location, CASE WHEN trim(coalesce(p.comment, '')) <> ''
                 AND substr(p.name, -length(trim(p.comment)) - 3) = ' (' || trim(p.comment) || ')'
            THEN substr(p.name, 1, length(p.name) - length(trim(p.comment)) - 3) ELSE p.name END),
       coalesce(p.full_location, CASE WHEN trim(coalesce(p.comment, '')) <> ''
                 AND substr(p.name, -length(trim(p.comment)) - 3) = ' (' || trim(p.comment) || ')'
            THEN substr(p.name, 1, length(p.name) - length(trim(p.comment)) - 3) ELSE p.name END),
       nullif(trim(p.familio_url), ''),
       CASE WHEN instr(p.familio_url, '/settlements/') > 0
            THEN nullif(trim(substr(p.familio_url, instr(p.familio_url, '/settlements/') + length('/settlements/')), '/ '), '') END
  FROM place p JOIN used u ON u.id = p.id
UNION ALL
-- Село дела без карточки пункта — одной строкой с названием: по нему образец
-- ищет место, и без строки колонка «Н.П. события» осталась бы без пары.
SELECT DISTINCT e.village, NULL, NULL, NULL, NULL, e.village, e.village, NULL, NULL
  FROM x_entry e JOIN chosen c ON c.id = e.id
 WHERE e.village IS NOT NULL AND e.village_place IS NULL
 ORDER BY 1, 7;

-- @excel_births
-- Лист «Рождения» (у индексатора «1»): 55 колонок A…BC, как у него.
SELECT c1, c2, c3, c4, c5, c6, c7, c8, c9, c10, c11, c12, c13, c14, c15, c16, c17, c18, c19, c20,
       c21, c22, c23, c24, c25, c26, c27, c28, c29, c30, c31, c32, c33, c34, c35, c36, c37, c38, c39, c40,
       c41, c42, c43, c44, c45, c46, c47, c48, c49, c50, c51, c52, c53, c54, c55
  FROM x_row WHERE section = 1 ORDER BY c1;

-- @excel_marriages
-- Лист «Браки» («2»): 55 колонок A…BC и ещё 8 — поручители 5 и 6 (BD…BK).
SELECT c1, c2, c3, c4, c5, c6, c7, c8, c9, c10, c11, c12, c13, c14, c15, c16, c17, c18, c19, c20,
       c21, c22, c23, c24, c25, c26, c27, c28, c29, c30, c31, c32, c33, c34, c35, c36, c37, c38, c39, c40,
       c41, c42, c43, c44, c45, c46, c47, c48, c49, c50, c51, c52, c53, c54, c55,
       c56, c57, c58, c59, c60, c61, c62, c63
  FROM x_row WHERE section = 2 ORDER BY c1;

-- @excel_deaths
-- Лист «Смерти» («3»): 55 колонок A…BC.
SELECT c1, c2, c3, c4, c5, c6, c7, c8, c9, c10, c11, c12, c13, c14, c15, c16, c17, c18, c19, c20,
       c21, c22, c23, c24, c25, c26, c27, c28, c29, c30, c31, c32, c33, c34, c35, c36, c37, c38, c39, c40,
       c41, c42, c43, c44, c45, c46, c47, c48, c49, c50, c51, c52, c53, c54, c55
  FROM x_row WHERE section = 3 ORDER BY c1;

-- @excel_mk
-- Лист «МК»: строка — одна персона одной записи (причт не персона, как у
-- индексатора). 78 колонок A…BZ: A–M — персона (Ф И О нормализованные,
-- отчество современное; у ребёнка отчество по имени отца), N…BP — строка
-- листа записи, BS…BZ — служебные.
WITH p AS (
    SELECT x.*, e.book_year, e.section, e.event_year, e.no_female, e.changed_at, e.church, e.village,
           CASE x.role_code
                WHEN 'child' THEN 1 WHEN 'father' THEN 2 WHEN 'mother' THEN 3
                WHEN 'godparent1' THEN 4 WHEN 'godparent2' THEN 5 WHEN 'godparent3' THEN 5 WHEN 'godparent4' THEN 5
                WHEN 'groom' THEN 6 WHEN 'bride' THEN 7
                WHEN 'witness1' THEN 8 WHEN 'witness2' THEN 9 WHEN 'witness3' THEN 10
                WHEN 'witness4' THEN 11 WHEN 'witness5' THEN 11 WHEN 'witness6' THEN 11
                WHEN 'deceased' THEN 12
                WHEN 'deceased_parent' THEN 13 WHEN 'deceased_spouse' THEN 14
                WHEN 'deceased_relative' THEN
                     CASE WHEN x.kinship IN ('отец', 'мать') THEN 13
                          WHEN x.kinship IN ('муж', 'жена', 'супруг', 'супруга') THEN 14 ELSE 15 END
                WHEN 'groom_relative' THEN 16
                WHEN 'bride_parent' THEN 17
                WHEN 'bride_relative' THEN CASE WHEN coalesce(x.kinship, 'отец') IN ('отец', 'мать') THEN 17 ELSE 18 END
           END AS code,
           (SELECT age_min FROM role r WHERE r.code = x.role_code) AS age_min,
           (SELECT age_max FROM role r WHERE r.code = x.role_code) AS age_max,
           (SELECT title FROM role r WHERE r.code = x.role_code) AS title,
           f.first_m AS father_first, f.surname_base AS father_surname, f.place AS father_place
      FROM x_person x
      JOIN x_entry e ON e.id = x.entry_id
      LEFT JOIN x_person f ON f.entry_id = x.entry_id AND f.role_code = 'father'
     WHERE x.role_code NOT LIKE 'clergy%'
       AND (x.iof_b <> '' OR x.role_code = 'deceased')
),
q AS (
    SELECT p.*,
           CASE WHEN p.role_code = 'child' THEN p.father_surname ELSE p.surname_base END AS fam0,
           CASE WHEN p.role_code = 'child' THEN
                (SELECT CASE WHEN coalesce(p.gender, CASE WHEN p.no_female IS NOT NULL THEN 'Ж' END) = 'Ж'
                             THEN d.patr_f ELSE d.patr_m END
                   FROM x_patr d WHERE d.name = p.father_first)
                ELSE p.patr_m END AS otch
      FROM p
),
r AS (
    SELECT q.*,
           -- Фамилия ребёнка — отцовская, у девочки по окончанию (Справочник!Q:R
           -- индексатора: ов → ова, ий → ая …).
           CASE WHEN q.role_code = 'child' AND coalesce(q.gender, CASE WHEN q.no_female IS NOT NULL THEN 'Ж' END) = 'Ж'
                     AND q.fam0 IS NOT NULL THEN
                CASE WHEN q.fam0 LIKE '%ий' OR q.fam0 LIKE '%ой' THEN substr(q.fam0, 1, length(q.fam0) - 2) || 'ая'
                     WHEN q.fam0 LIKE '%ов' OR q.fam0 LIKE '%ев' OR q.fam0 LIKE '%ин' OR q.fam0 LIKE '%ын' THEN q.fam0 || 'а'
                     ELSE q.fam0 END
                ELSE q.fam0 END AS fam
      FROM q
),
s AS (
    SELECT r.*, nullif(trim(coalesce(r.fam || ' ', '') || coalesce(r.first_m || ' ', '') || coalesce(r.otch, '')), '') AS fio
      FROM r
)
SELECT row_number() OVER (ORDER BY s.book_year, s.section, s.entry_id, s.sort_order, s.id),
       s.book_year, s.code,
       CASE WHEN s.role_code = 'deceased_relative' THEN 'умершего ' || coalesce(s.kinship, 'родственник')
            WHEN s.role_code = 'groom_relative' THEN 'родственник жениха ' || coalesce(s.kinship, '')
            WHEN s.role_code IN ('bride_relative', 'bride_parent') AND s.code = 17 THEN 'родитель невесты ' || coalesce(s.kinship, 'отец')
            WHEN s.role_code IN ('bride_relative', 'bride_parent') THEN 'родственник невесты ' || coalesce(s.kinship, '')
            ELSE s.title END,
       -- Рождение: у ребёнка — дата; у умершего, жениха, невесты — год по
       -- возрасту; у остальных — коридор по роли (отец 17–55 лет).
       CASE WHEN s.role_code = 'child' THEN xr.c10
            WHEN s.role_code = 'deceased' THEN
                 CASE WHEN s.age_years IS NOT NULL THEN s.event_year - s.age_years
                      WHEN coalesce(s.age_months, s.age_weeks, s.age_days) IS NOT NULL THEN s.event_year END
            WHEN s.role_code IN ('groom', 'bride') AND s.age_years IS NOT NULL THEN s.book_year - s.age_years
            WHEN s.age_min IS NOT NULL AND s.age_max IS NOT NULL
                 THEN (s.book_year - s.age_max) || '-' || (s.book_year - s.age_min) END,
       CASE WHEN s.role_code = 'deceased' THEN xr.c10 END,
       s.fio,
       CASE WHEN s.fio IS NOT NULL THEN count(*) OVER (PARTITION BY s.fio) END,
       s.fam, s.first_m, s.otch,
       CASE WHEN s.role_code = 'child' THEN
            CASE coalesce(s.gender, CASE WHEN s.no_female IS NOT NULL THEN 'Ж' ELSE 'М' END) WHEN 'Ж' THEN 'дочь' ELSE 'сын' END
            ELSE s.rank END,
       CASE WHEN s.role_code = 'child' THEN s.father_place ELSE s.place END,
       xr.c1, xr.c2, xr.c3, xr.c4, xr.c5, xr.c6, xr.c7, xr.c8, xr.c9, xr.c10,
       xr.c11, xr.c12, xr.c13, xr.c14, xr.c15, xr.c16, xr.c17, xr.c18, xr.c19, xr.c20,
       xr.c21, xr.c22, xr.c23, xr.c24, xr.c25, xr.c26, xr.c27, xr.c28, xr.c29, xr.c30,
       xr.c31, xr.c32, xr.c33, xr.c34, xr.c35, xr.c36, xr.c37, xr.c38, xr.c39, xr.c40,
       xr.c41, xr.c42, xr.c43, xr.c44, xr.c45, xr.c46, xr.c47, xr.c48, xr.c49, xr.c50,
       xr.c51, xr.c52, xr.c53, xr.c54, xr.c55,
       NULL, NULL,
       'GM-' || s.entry_id, s.sort_order, s.church, s.village,
       CASE WHEN s.role_code = 'mother' THEN s.maiden END,
       1, 1, s.changed_at
  FROM s JOIN x_row xr ON xr.entry_id = s.entry_id
 ORDER BY s.book_year, s.section, s.entry_id, s.sort_order, s.id;

-- @export_parish
-- Село дела — для имени файла выгрузки.
-- Чистое название: комментарий деревни-тёзки в имя файла не идёт.
SELECT coalesce((SELECT CASE WHEN trim(coalesce(p.comment, '')) <> ''
                 AND substr(p.name, -length(trim(p.comment)) - 3) = ' (' || trim(p.comment) || ')'
            THEN substr(p.name, 1, length(p.name) - length(trim(p.comment)) - 3) ELSE p.name END
                   FROM place p WHERE p.name = trim(c.village) LIMIT 1), c.village, '')
  FROM mk_case c ORDER BY c.updated_at DESC, c.id DESC LIMIT 1;
