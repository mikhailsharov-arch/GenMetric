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
-- Населённый пункт заводится по первому упоминанию — запасной путь, если
-- карточка (place_save) почему-то не открылась и НП дошёл до сохранения
-- записи неизвестным. Подробности тогда пусты, дозаполняются позже.
INSERT INTO place (name, name_norm, origin) VALUES (:name, :name_norm, 'user');

-- @place_save
-- Карточка населённого пункта при первом вводе (Роман, приоритет 2 от
-- 23.09.2026): губерния и уезд по умолчанию из дела, тип, волость, ссылка
-- на Familio. short/full_location собираются здесь же — как в place.csv
-- из Excel, чтобы выгрузка не различала свои и перенесённые места.
INSERT INTO place (name, name_norm, np_type, guberniya, uyezd, volost,
                   short_location, full_location, familio_url, origin)
VALUES (:name, :name_norm, :np_type, :guberniya, :uyezd, :volost,
        trim(coalesce(:np_type, '') || ' ' || :name),
        trim(coalesce(:np_type, '') || ' ' || :name)
          || CASE WHEN :volost    IS NULL OR :volost    = '' THEN '' ELSE ', ' || :volost    || ' волость'  END
          || CASE WHEN :uyezd     IS NULL OR :uyezd     = '' THEN '' ELSE ', ' || :uyezd     || ' уезд'     END
          || CASE WHEN :guberniya IS NULL OR :guberniya = '' THEN '' ELSE ', ' || :guberniya || ' губерния' END,
        :familio_url, 'user');

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
                  AND c.first_name IS NULL AND c.surname IS NULL AND c.rank IS NOT NULL) AS clergy_noname
  FROM entry e
 WHERE e.case_id = :case_id AND e.section = :section
 ORDER BY e.id DESC;

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
       m.note, m.uncertain
  FROM person_mention m
 WHERE m.entry_id = :entry_id
 ORDER BY m.sort_order;

-- @last_clergy
-- Причт последней записи дела — чтобы после перезапуска форма продолжала
-- с ним, как со страницей и счётом (заказчик 21.09.2026: «надо сделать,
-- чтобы церковнослужители также сохранялись»).
SELECT m.role_code,
       -- Без отчества между именем и фамилией остался бы двойной пробел.
       trim(coalesce(m.first_name, '')
            || CASE WHEN m.patronymic IS NULL THEN '' ELSE ' ' || m.patronymic END
            || CASE WHEN m.surname IS NULL THEN '' ELSE ' ' || m.surname END) AS iof,
       m.rank, m.note
  FROM person_mention m
 WHERE m.entry_id = (SELECT max(e.id) FROM entry e WHERE e.case_id = :case_id AND e.section = :section)
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
-- Порядок выдачи по требованию А-1: текущее дело, затем приход, затем вся
-- база, затем словарь; внутри группы — по убыванию частоты.
WITH ranked AS (
    SELECT value,
           CASE scope WHEN 'case' THEN 1 WHEN 'parish' THEN 2 ELSE 3 END AS tier,
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

-- @suggest_first_name
-- Имя по полу роли: матери — женские, отцу — мужские. Заказчик 13.09.2026:
-- «при вводе ИОФ матери подставляет мужские имена». Пол неизвестен — обе.
SELECT form, 4, 0 FROM name_form
 WHERE kind IN ('name','variant') AND form_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender = :gender)

-- @suggest_patronymic
-- Отчество мужчины и отчество женщины — разные формы одного имени. Пол берётся
-- из роли персоны: отец всегда мужчина, мать всегда женщина. Если пол
-- неизвестен (ребёнок, восприемник), показываются обе формы.
SELECT form, 4, 0 FROM name_form
 WHERE kind LIKE 'patr%' AND form_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender = :gender)

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
INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses, last_used_at)
VALUES (:iof, :iof_norm, :place, :rank, :gender, 1, datetime('now'))
ON CONFLICT(iof, place, rank) DO UPDATE SET
    uses = uses + 1, last_used_at = datetime('now'),
    gender = coalesce(excluded.gender, gender);

-- @person_suggest
-- Подсказка персонами. Сначала те, кого вводили чаще: заказчик работает
-- приходами, и одни и те же люди возвращаются в записях год за годом.
-- Персоны — тоже по полу роли. Пол у персоны может быть не записан (старые
-- записи), такие показываются всем: лучше лишняя строка, чем потерянный человек.
SELECT iof, place, rank, gender, uses
  FROM person_index
 WHERE iof_norm LIKE :prefix ESCAPE '\'
   AND (:gender IS NULL OR gender IS NULL OR gender = :gender)
 ORDER BY uses DESC, iof
 LIMIT :limit;

-- @clergy_remember
-- Причт запоминается, чтобы его можно было выбирать, а не набирать. Заказчик
-- 27.08.2026: «если в списке его нет, то после ввода его руками он добавляется
-- в базу и появляется в списке».
INSERT INTO clergy_index (iof, iof_norm, rank, uses, last_used_at)
VALUES (:iof, :iof_norm, :rank, 1, datetime('now'))
ON CONFLICT(iof, rank) DO UPDATE SET
    uses = uses + 1, last_used_at = datetime('now');

-- @clergy_list
-- Весь список целиком, без ввода первых букв: причт в приходе меняется редко,
-- за год-два это те же три человека.
SELECT iof, rank, uses FROM clergy_index
 ORDER BY uses DESC, last_used_at DESC, iof
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
