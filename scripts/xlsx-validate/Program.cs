// Проверка файла xlsx валидатором Open XML SDK (Microsoft).
//
// Зачем. 01.10.2026 Роман открыл выгрузку в Familio в Excel и получил «Ошибка в
// части содержимого… Выполнить попытку восстановления?». Excel у нас нет; файл
// проверялся openpyxl и Numbers, а они прощают то, что Excel считает
// повреждением. Этот валидатор проверяет файл по схеме формата — тем же
// правилам, по которым его читает Excel.
//
// Запуск:  dotnet run --project scripts/xlsx-validate -- файл.xlsx [образец.xlsx]
// С образцом печатаются только ошибки, которых нет в самом образце: шаблон
// Familio сохранён Excel и содержит расширения новее схемы — это шум.
// Код возврата 1, если новые ошибки есть.
using DocumentFormat.OpenXml;
using DocumentFormat.OpenXml.Packaging;
using DocumentFormat.OpenXml.Validation;

static List<string> Check(string path)
{
    var found = new List<string>();
    try
    {
        using var doc = SpreadsheetDocument.Open(path, false);
        var validator = new OpenXmlValidator(FileFormatVersions.Microsoft365);
        foreach (var e in validator.Validate(doc))
        {
            var part = e.Part?.Uri.ToString() ?? "?";
            // Без номера строки листа: «/row[5]» и «/row[6]» — одна и та же беда.
            var where = System.Text.RegularExpressions.Regex.Replace(e.Path?.XPath ?? "", @"\[\d+\]", "[]");
            found.Add($"{part} | {where} | {e.Description}");
        }
    }
    catch (Exception ex)
    {
        found.Add($"файл не открылся: {ex.GetType().Name}: {ex.Message}");
    }
    return found;
}

// На Windows консоль по умолчанию не в UTF-8 — кириллица превратилась бы в «?».
Console.OutputEncoding = System.Text.Encoding.UTF8;

if (args.Length < 1)
{
    Console.WriteLine("запуск: xlsx-validate файл.xlsx [образец.xlsx]");
    return 2;
}
var errors = Check(args[0]);
var baseline = args.Length > 1 ? Check(args[1]).ToHashSet() : new HashSet<string>();
var fresh = errors.Where(e => !baseline.Contains(e)).Distinct().ToList();
Console.WriteLine($"{Path.GetFileName(args[0])}: ошибок {errors.Count}, из них новых (нет в образце) {fresh.Count}");
foreach (var e in fresh.Take(60)) Console.WriteLine("  [ОШИБКА] " + e);
return fresh.Count > 0 ? 1 : 0;
