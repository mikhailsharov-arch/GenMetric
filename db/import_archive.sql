-- ============================================================================
--  Слияние архива подсказок в базу пользователя.
--
--  Архив собирается инструментом db/tools/build_archive.py из рабочего файла
--  Excel-индексатора и содержит только память: персон, их места и звания,
--  связки «муж — жена», причт, частоту употребления. К моменту выполнения
--  архив подключён под именем archive.
--
--  ПРАВИЛО: только дополняем. Ничего набранного в программе не удаляется
--  и не перезаписывается. Где записи совпадают — счётчики складываются,
--  чтобы порядок подсказок учитывал и Excel, и программу.
--
--  Повторная загрузка того же архива не должна задваивать строки: совпадения
--  ищутся по нормализованным ключам с NULL-безопасным IS, там где ключ
--  допускает пустоту, и через ON CONFLICT там, где не допускает. Счётчики
--  при повторе сложатся ещё раз — поэтому в setting запоминается отпечаток
--  загруженного архива, и приложение не даёт загрузить тот же дважды.
--
--  Тот же файл прогоняет db/test_archive.py.
-- ============================================================================

BEGIN;

-- Персоны: ИОФ + место + звание. Совпали — складываем частоту, пол берём,
-- если своего не было.
--
-- Сверка через IS, а не через ON CONFLICT: у SQLite NULL в уникальном ключе
-- никогда не равен NULL, и персона без места или звания задваивалась бы
-- при каждой загрузке. Поймано тестом на повторное слияние.
UPDATE person_index SET
    uses = uses + (SELECT sum(a.uses) FROM archive.person_index a
                    WHERE a.iof_norm = person_index.iof_norm
                      AND coalesce(a.place, '') = coalesce(person_index.place, '') AND coalesce(a.rank, '') = coalesce(person_index.rank, '')),
    gender = coalesce(gender, (SELECT max(a.gender) FROM archive.person_index a
                                WHERE a.iof_norm = person_index.iof_norm
                                  AND coalesce(a.place, '') = coalesce(person_index.place, '') AND coalesce(a.rank, '') = coalesce(person_index.rank, '')))
 WHERE EXISTS (SELECT 1 FROM archive.person_index a
                WHERE a.iof_norm = person_index.iof_norm
                  AND coalesce(a.place, '') = coalesce(person_index.place, '') AND coalesce(a.rank, '') = coalesce(person_index.rank, ''));

INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses, last_used_at)
-- Пустое место или звание — пустой строкой, как пишет person_remember
-- (техдолг В6, 27.09.2026); в старом архиве NULL и '' могут стоять у одного
-- человека — группировка сливает их, иначе вставка упала бы на UNIQUE.
SELECT a.iof, min(a.iof_norm), coalesce(a.place, ''), coalesce(a.rank, ''), max(a.gender),
       sum(a.uses), max(a.last_used_at)
  FROM archive.person_index a
 WHERE NOT EXISTS (SELECT 1 FROM person_index p
                    WHERE p.iof_norm = a.iof_norm AND coalesce(p.place, '') = coalesce(a.place, '')
                      AND coalesce(p.rank, '') = coalesce(a.rank, ''))
 GROUP BY a.iof, coalesce(a.place, ''), coalesce(a.rank, '');

-- Жёны по мужьям.
INSERT INTO spouse_index (husband_norm, wife_iof, wife_place, wife_rank, uses, last_used_at)
SELECT husband_norm, wife_iof, wife_place, wife_rank, uses, last_used_at FROM archive.spouse_index
WHERE true
ON CONFLICT(husband_norm, wife_iof) DO UPDATE SET
    uses = spouse_index.uses + excluded.uses,
    wife_place = coalesce(spouse_index.wife_place, excluded.wife_place),
    wife_rank = coalesce(spouse_index.wife_rank, excluded.wife_rank);

-- Причт. Та же оговорка про NULL в звании; пустое звание — пустой строкой.
UPDATE clergy_index SET
    uses = uses + (SELECT sum(a.uses) FROM archive.clergy_index a
                    WHERE a.iof_norm = clergy_index.iof_norm
                      AND coalesce(a.rank, '') = coalesce(clergy_index.rank, ''))
 WHERE EXISTS (SELECT 1 FROM archive.clergy_index a
                WHERE a.iof_norm = clergy_index.iof_norm
                  AND coalesce(a.rank, '') = coalesce(clergy_index.rank, ''));

INSERT INTO clergy_index (iof, iof_norm, rank, uses, last_used_at)
SELECT a.iof, min(a.iof_norm), coalesce(a.rank, ''), sum(a.uses), max(a.last_used_at)
  FROM archive.clergy_index a
 WHERE NOT EXISTS (SELECT 1 FROM clergy_index c
                    WHERE c.iof_norm = a.iof_norm AND coalesce(c.rank, '') = coalesce(a.rank, ''))
 GROUP BY a.iof, coalesce(a.rank, '');

-- Населённые пункты: только те, которых ещё нет. Сверка по нормализованному
-- имени, а не по UNIQUE-ключу таблицы: у архива нет губернии и уезда, и одно
-- название не должно заводиться второй раз без них.
INSERT INTO place (name, name_norm, origin)
SELECT a.name, a.name_norm, 'archive'
  FROM archive.place a
 WHERE NOT EXISTS (SELECT 1 FROM place p WHERE p.name_norm = a.name_norm);

-- Частота употребления: складываем.
INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)
SELECT kind, scope, scope_key, value, value_norm, count, last_used_at FROM archive.usage_stat
WHERE true
ON CONFLICT(kind, scope, scope_key, value) DO UPDATE SET
    count = usage_stat.count + excluded.count;

-- Отметка о загрузке: откуда и когда. По ней приложение отказывает
-- в повторной загрузке того же архива.
INSERT INTO setting (key, value)
SELECT 'archive_loaded', coalesce((SELECT value FROM archive.setting WHERE key = 'archive_source'), '?')
    || ' @ ' || coalesce((SELECT value FROM archive.setting WHERE key = 'archive_built'), '?')
ON CONFLICT(key) DO UPDATE SET value = excluded.value;

COMMIT;
