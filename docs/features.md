# Funktionsliste

Alle Funktionen von CuneiEdit Menü für Menü, mit Stand nach Welle 1 bis 3 (2026-11-05).
Alles ist eigenständig umgesetzt (Clean-Room, siehe `CONTRIBUTING.md`).

**Legende:** ✅ vorhanden · 🟡 teilweise · ⬜ fehlt · 🍎 auf dem Mac nicht sinnvoll / anders gelöst ·
⚠️ widerspricht einer Sicherheitsregel (Netzwerk, externe Programme, nachgeladener Code) – nur nach Entscheidung ·
**W2/W3** = Welle, in der ein offener Punkt umgesetzt werden soll

## Datei

| Funktion | Status | Anmerkung |
|---|---|---|
| Neu, Öffnen, Speichern, Speichern unter, Schließen, Beenden | ✅ | |
| Zuletzt geöffnet (Untermenü, alle öffnen, Liste leeren) | ✅ | |
| Zuletzt geschlossene Datei wiederherstellen | ✅ | ⌘⇧T, merkt sich 20 Dateien |
| Alle speichern, Kopie speichern unter | ✅ | |
| Alle schließen / alle außer aktivem / links / rechts / unveränderte | ✅ | angeheftete Tabs bleiben offen |
| Tabs anheften | ✅ | |
| Von Festplatte neu laden | ✅ | rückgängig machbar |
| Umbenennen | ✅ | im selben Ordner |
| In den Papierkorb legen | ✅ | eigene Umsetzung ohne Abhängigkeit: macOS `~/.Trash`, Linux freedesktop-Papierkorb; Windows fehlt (braucht Shell-API, also `unsafe` oder eine Abhängigkeit – Entscheidung offen); „Zurücklegen“ im Finder geht nicht |
| Im Finder zeigen | ✅ | erlaubte Ausnahme: fester Aufruf `/usr/bin/open -R` |
| Öffnen aus dem Finder („Öffnen mit“, Doppelklick, aufs Dock-Symbol ziehen) | ✅ | macOS, über `cuneiedit-macos`; als Alternative, nicht als Standard-Programm registriert |
| Nur eine Instanz | ✅ | Dateien aus Explorer, Finder oder Kommandozeile öffnen sich im laufenden Fenster; lokal und nur für den Benutzer (Unix-Socket im privaten Ordner, unter Windows Named Pipe) |
| Native Mac-Menüleiste | ✅ | Programm-, Ablage-, Bearbeiten-, Darstellungs-, Fenster- und Hilfe-Menü mit den wichtigsten Befehlen (`muda`); vollständige Menüs im Fenster |
| Ordner im Terminal öffnen, in Standard-Programm öffnen | ⚠️ | externer Prozess |
| Ordner als Arbeitsbereich (Dateibaum-Seitenleiste) | ✅ | liest Ordner erst beim Aufklappen, folgt keinen symbolischen Links auf Ordner |
| Sitzung laden / speichern | ✅ | automatisch (mit ungespeichertem Text) und als Datei (nur Pfade) |
| Drucken | ✅ | macOS: Druckdialog mit Vorschau (PDFKit), die Schriften des Editors werden eingebettet (Unicode wie im Editor, fehlende Zeichen als Kästchen); Windows: Druckdialog (Drucker, Seiten, Kopien), Druck mit Consolas über GDI; beides mit Syntaxfarben; Linux speichert ein PDF |
| Exportieren als PDF | ✅ | |
| Exportieren als HTML | ✅ | mit Syntaxfarben des Farbschemas |

## Bearbeiten

| Funktion | Status | Anmerkung |
|---|---|---|
| Rückgängig, Wiederholen, Ausschneiden, Kopieren, Einfügen, Alles auswählen | ✅ | |
| Auswahl beginnen/beenden, Spaltenmodus, Spalteneditor | ✅ | Alt+Ziehen wählt Spalten, Spalteneditor für Text und Zahlenfolgen (Dez/Hex/Okt/Bin) |
| Datum/Uhrzeit einfügen (kurz, lang, eigenes Format) | ✅ | Format unter „Alle Einstellungen“; Wochentage nur englisch |
| Dateipfad, Dateiname, Ordnerpfad kopieren | ✅ | |
| Einzug vergrößern / verkleinern | ✅ | Menü und Tab/⇧Tab |
| Groß/Klein: alle 8 Varianten (inkl. „blend“, umkehren, zufällig) | ✅ | |
| Zeile duplizieren, löschen, verschieben | ✅ | |
| Doppelte / aufeinanderfolgende doppelte Zeilen entfernen | ✅ | |
| Zeilen teilen (an Randspalte) / verbinden | ✅ | |
| Sortieren: lexikografisch, ohne Groß/Klein, Sprachregeln, Länge, Ganzzahl, Dezimal (Komma/Punkt), je auf/ab | ✅ | Zeilen ohne Zahl werden gemeldet |
| Zeilenreihenfolge umkehren / zufällig | ✅ | |
| Leere Zeilen entfernen (auch mit Leerraum), Leerzeile ober-/unterhalb | ✅ | |
| Kommentar: umschalten, ein, aus; Blockkommentar ein/aus | ✅ | |
| Autovervollständigung: Wörter und Schlüsselwörter | ✅ | beim Tippen (abschaltbar) und ⌃Leertaste |
| Autovervollständigung: Funktionsparameter, Pfade | ✅ | Aufruf-Tipp aus den Definitionen im Dokument nach `(`; Pfade mit ⌃Leertaste |
| Zeilenende umwandeln | ✅ | Menü „Kodierung“ |
| Leerraum: alle Varianten (Anfang, Ende, beides, Zeilenende → Leerzeichen, Tab ↔ Leerzeichen) | ✅ | |
| Auswahl schwärzen | ✅ | mit █ |
| Mehrfachauswahl / Mehrfachcursor | ✅ | ⌘/Strg+Klick, ⌥⇧↑/↓; Tippen, Löschen, Einfügen (zeilenweise verteilt), Pfeiltasten; Esc lässt einen Cursor |
| Zeichentabelle | ✅ | Seitenleiste, Klick fügt ein |
| Zwischenablage-Verlauf | ✅ | nur im Speicher, nur solange die Leiste offen ist |
| Schreibschutz pro Dokument | ✅ | |
| Spezielles Einfügen (HTML/RTF/Binär) | 🍎 | Windows-Formate |
| Textrichtung RTL/LTR | ⬜ | egui kann keinen bidirektionalen Text setzen – erst mit Unterstützung in egui |

## Suchen

| Funktion | Status | Anmerkung |
|---|---|---|
| Suchen, Ersetzen, Weitersuchen, Rückwärts, inkrementell | ✅ | Suchleiste |
| Groß/Klein, ganzes Wort, regulärer Ausdruck, erweitert (`\n \t \x…`), „. findet Zeilenumbruch“ | ✅ | |
| In Auswahl, Umbruch am Dokumentende abschaltbar | ✅ | |
| Alle im Dokument / in allen offenen Dokumenten finden | ✅ | Ergebnisfenster (F7) |
| In allen offenen Dokumenten ersetzen | ✅ | Suchleiste, schreibgeschützte Dokumente bleiben unverändert |
| Suchen / Ersetzen in Dateien (Filter mit `!`-Ausschluss, Unterordner, versteckte) | ✅ | im Hintergrund, abbrechbar; Ersetzen nur nach Bestätigung; Binärdateien und > 64 MB werden übersprungen |
| In Ergebnissen suchen, nächstes Ergebnis per Taste | ✅ | Filterfeld im Ergebnisfenster, F4 / ⇧F4 |
| Auswählen und weitersuchen / rückwärts | ✅ | ⌘F3 / ⌘⇧F3 |
| Gehe zu Zeile / Position | ✅ | |
| Zur passenden Klammer, Auswahl zwischen Klammern | ✅ | ⌘B / ⌘⇧B |
| Markieren mit 5 Stilen, Stil entfernen | ✅ | folgt Bearbeitungen (wird neu gesucht) |
| Zu Markierung springen, markierten Text kopieren | ✅ | |
| Lesezeichen: alle Funktionen inkl. ausschneiden und durch Zwischenablage ersetzen | ✅ | |
| Zeichen in Bereich suchen | ✅ | |
| Änderungsverlauf im Rand (ungespeichert orange, gespeichert grün) | ✅ | bis 100 000 Zeilen; zur nächsten/vorherigen Änderung springen |

## Ansicht

| Funktion | Status | Anmerkung |
|---|---|---|
| Leerzeichen/Tabs, Zeilenenden, alle Zeichen, Einrückungslinien, Umbruchsymbol, Steuerzeichen | ✅ | |
| Zeilenumbruch | ✅ | |
| Zoom | ✅ | |
| Symbolleiste | ✅ | eigene, im Code gezeichnete Symbole; abschaltbar |
| Immer im Vordergrund, Vollbild | ✅ | F11 |
| Ablenkungsfreier Modus, Post-it | ✅ | ⌘⇧D bzw. F12, Esc beendet |
| Code-Folding, Zeilen ausblenden | ✅ | nach Klammern oder Einrückung; Faltmarken im Rand, alle/aktuelle Ebene ein- und ausklappen |
| Zusammenfassung | ✅ | |
| Funktionsliste, Dokumentliste | ✅ | |
| Dokumentkarte | ✅ | Miniatur mit Syntaxfarben, Klick und Ziehen scrollen |
| Geteilte Ansicht (zweite Ansicht desselben Dokuments) | ✅ | dazu „In andere Ansicht verschieben“: zwei Dokumente nebeneinander |
| Tabs: ⌘1–9, nächster/vorheriger, verschieben, Farbe | ✅ | |
| Überwachen (`tail -f`) | ✅ | schreibgeschützt, lädt automatisch neu |
| Aktuelle Datei im Browser | ⚠️ | externer Prozess |

## Kodierung

| Funktion | Status | Anmerkung |
|---|---|---|
| UTF-8, UTF-8 BOM, UTF-16 LE/BE, Windows-1252 | ✅ | |
| „Interpretieren als“ und „Konvertieren in“ | ✅ | |
| 32 weitere Zeichensätze nach Schrift gruppiert | ✅ | |
| DOS-Codepages (OEM 437, 850 …) | 🟡 | 437, 850, 858 (eigene Tabellen) und 866; weitere OEM-Seiten fehlen |
| Erkennung über BOM/UTF-8 hinaus | 🟡 | zusätzlich UTF-16 ohne BOM; andere Kodierungen werden nicht erraten (Rückfall Windows-1252) |

## Sprache (Syntax)

| Funktion | Status | Anmerkung |
|---|---|---|
| Sprachauswahl, Erkennung über Endung | ✅ | 60 Sprachen, im Menü nach Anfangsbuchstaben gruppiert; Funktionen, Escape-Sequenzen, Interpolation, Regex-Literale, JavaScript/CSS in HTML |
| Benutzerdefinierte Sprachen | ✅ | Schlüsselwörter, Typen, Kommentare, Zeichenketten, Endungen, Groß/Klein; ohne Faltregeln und Operatoren |

## Einstellungen

| Funktion | Status | Anmerkung |
|---|---|---|
| Oberflächensprache, Erscheinungsbild, Tabbreite, Einrücken mit Leerzeichen | ✅ | |
| Klammern/Anführungszeichen automatisch schließen, Autovervollständigung | ✅ | |
| Neue Dokumente: Kodierung und Zeilenende | ✅ | „Alle Einstellungen“ |
| Randspalte, Datumsformat | ✅ | |
| Ungespeicherte Änderungen beim Beenden merken (Sitzungs-Schnappschuss) | ✅ | Standard an; zusätzlich Sicherung alle 10 s; Ordner nur für den Benutzer lesbar |
| Externe Änderungen erkennen (fragen / still neu laden) | ✅ | Prüfung alle 2 s |
| Vollständiger Einstellungsdialog, Farbthemen, Schriftart, Tastenkürzel anpassen | ✅ | 5 Farbschemata, eigene Schriftdatei, Tastenkürzel per Tastendruck; Kürzel des Editors (Rückgängig, Zwischenablage, Tab) fest |
| Sicherung beim Speichern (Kopie mit Zeitstempel) | ✅ | einfach (`name.bak`) oder ausführlich (mit Datum im Einstellungsordner) |
| Alle Vorkommen des markierten Worts hervorheben, passende HTML-Tags | ✅ | im sichtbaren Bereich; Tags in HTML/XML |
| Updates | ✅ | Frage beim ersten Start, Einstellung (aus / benachrichtigen / automatisch), „Nach Updates suchen …“; signiert (Ed25519) von GitHub; macOS und Windows installieren selbst (Admin-Passwort bzw. UAC wenn nötig), Linux zeigt die Release-Seite |
| Klickbare Links, Internetsuche, Plugins | ⚠️ | |

## Werkzeuge

| Funktion | Status | Anmerkung |
|---|---|---|
| MD5, SHA-1, SHA-256, SHA-512: aus Auswahl in Zwischenablage, aus Dateien | ✅ | RustCrypto |

## Makro

| Funktion | Status | Anmerkung |
|---|---|---|
| Aufnahme starten/beenden, abspielen, mehrfach / bis Dateiende | ✅ | ⌘⇧R / ⌘⇧P; zeichnet Tastatureingaben auf, keine Menübefehle |
| Makros speichern und verwalten | ✅ | mit Namen im Menü „Makro“, löschen; Datei `macros.ini` |

## Ausführen, Plugins

| Funktion | Status | Anmerkung |
|---|---|---|
| Externe Programme, Plugins | ⚠️ | ausgeschlossen |
| Vergleichen | ✅ | eingebaut |

## Tab-Kontextmenü

| Funktion | Status | Anmerkung |
|---|---|---|
| Schließen-Varianten, Speichern, Umbenennen, Neu laden, Im Finder zeigen, Pfad kopieren, Schreibschutz, Anheften, Farbe, Papierkorb | ✅ | |
| In andere Ansicht / in neues Fenster verschieben | ✅ | neues Fenster = eigene Instanz, nur für gespeicherte Dateien; ohne Sitzung und Sicherungen |
| Tabs per Ziehen umsortieren | ✅ | |

## Welle 3 (erledigt bis auf zwei Punkte)

Erledigt: Drucken (über PDF), Mehrfachcursor/Spaltenmodus, Code-Folding/Zeilen ausblenden,
Dokumentkarte, Dateibaum, Einstellungsdialog mit Farbschemata/Schriftart/Tastenkürzeln,
Ersetzen in allen offenen Dokumenten, Makros speichern, eigene Sprachen, DOS-Codepages, Tabs ziehen und
alle übrigen W3-Zeilen.

Offen: **RTL** (egui kann keinen bidirektionalen Text) und **Papierkorb unter Windows**
(braucht `unsafe` oder eine Abhängigkeit – Entscheidung nötig).
