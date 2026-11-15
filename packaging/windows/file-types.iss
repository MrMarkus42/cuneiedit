; Generated from crates/cuneiedit-app/src/file_types.rs – do not edit by hand.

[CustomMessages]
english.FileTypes=Open these file types with CuneiEdit
english.FileGroup_text=Text and documentation
english.FileGroup_config=Configuration
english.FileGroup_data=Data
english.FileGroup_code=Source code
english.FileGroup_web=Stylesheets and components
english.FileGroup_pages=Web pages and images (usually opened in the browser)
english.FileGroup_scripts=Scripts (a double click runs them otherwise)
german.FileTypes=Diese Dateitypen mit CuneiEdit öffnen
german.FileGroup_text=Text und Dokumentation
german.FileGroup_config=Konfiguration
german.FileGroup_data=Daten
german.FileGroup_code=Quellcode
german.FileGroup_web=Stylesheets und Komponenten
german.FileGroup_pages=Webseiten und Bilder (öffnen sonst im Browser)
german.FileGroup_scripts=Skripte (ein Doppelklick führt sie sonst aus)

[Tasks]
Name: "types"; Description: "{cm:FileTypes}"
Name: "types\text"; Description: "{cm:FileGroup_text}"
Name: "types\text\ext_txt"; Description: ".txt"
Name: "types\text\ext_text"; Description: ".text"
Name: "types\text\ext_log"; Description: ".log"
Name: "types\text\ext_nfo"; Description: ".nfo"
Name: "types\text\ext_md"; Description: ".md"
Name: "types\text\ext_markdown"; Description: ".markdown"
Name: "types\text\ext_rst"; Description: ".rst"
Name: "types\text\ext_adoc"; Description: ".adoc"
Name: "types\text\ext_tex"; Description: ".tex"
Name: "types\config"; Description: "{cm:FileGroup_config}"
Name: "types\config\ext_ini"; Description: ".ini"
Name: "types\config\ext_cfg"; Description: ".cfg"
Name: "types\config\ext_conf"; Description: ".conf"
Name: "types\config\ext_config"; Description: ".config"
Name: "types\config\ext_toml"; Description: ".toml"
Name: "types\config\ext_yaml"; Description: ".yaml"
Name: "types\config\ext_yml"; Description: ".yml"
Name: "types\config\ext_json"; Description: ".json"
Name: "types\config\ext_jsonc"; Description: ".jsonc"
Name: "types\config\ext_properties"; Description: ".properties"
Name: "types\config\ext_editorconfig"; Description: ".editorconfig"
Name: "types\config\ext_gitignore"; Description: ".gitignore"
Name: "types\config\ext_gitattributes"; Description: ".gitattributes"
Name: "types\config\ext_env"; Description: ".env"
Name: "types\data"; Description: "{cm:FileGroup_data}"
Name: "types\data\ext_csv"; Description: ".csv"
Name: "types\data\ext_tsv"; Description: ".tsv"
Name: "types\data\ext_xml"; Description: ".xml"
Name: "types\data\ext_xsd"; Description: ".xsd"
Name: "types\data\ext_xsl"; Description: ".xsl"
Name: "types\data\ext_sql"; Description: ".sql"
Name: "types\data\ext_diff"; Description: ".diff"
Name: "types\data\ext_patch"; Description: ".patch"
Name: "types\code"; Description: "{cm:FileGroup_code}"
Name: "types\code\ext_c"; Description: ".c"
Name: "types\code\ext_h"; Description: ".h"
Name: "types\code\ext_cpp"; Description: ".cpp"
Name: "types\code\ext_cc"; Description: ".cc"
Name: "types\code\ext_cxx"; Description: ".cxx"
Name: "types\code\ext_hpp"; Description: ".hpp"
Name: "types\code\ext_hh"; Description: ".hh"
Name: "types\code\ext_cs"; Description: ".cs"
Name: "types\code\ext_java"; Description: ".java"
Name: "types\code\ext_kt"; Description: ".kt"
Name: "types\code\ext_kts"; Description: ".kts"
Name: "types\code\ext_go"; Description: ".go"
Name: "types\code\ext_rs"; Description: ".rs"
Name: "types\code\ext_swift"; Description: ".swift"
Name: "types\code\ext_php"; Description: ".php"
Name: "types\code\ext_lua"; Description: ".lua"
Name: "types\code\ext_dart"; Description: ".dart"
Name: "types\code\ext_scala"; Description: ".scala"
Name: "types\code\ext_groovy"; Description: ".groovy"
Name: "types\code\ext_gradle"; Description: ".gradle"
Name: "types\code\ext_cmake"; Description: ".cmake"
Name: "types\code\ext_asm"; Description: ".asm"
Name: "types\code\ext_pas"; Description: ".pas"
Name: "types\code\ext_fs"; Description: ".fs"
Name: "types\code\ext_hs"; Description: ".hs"
Name: "types\code\ext_ex"; Description: ".ex"
Name: "types\code\ext_exs"; Description: ".exs"
Name: "types\code\ext_erl"; Description: ".erl"
Name: "types\code\ext_zig"; Description: ".zig"
Name: "types\code\ext_jl"; Description: ".jl"
Name: "types\code\ext_r"; Description: ".r"
Name: "types\web"; Description: "{cm:FileGroup_web}"
Name: "types\web\ext_css"; Description: ".css"
Name: "types\web\ext_scss"; Description: ".scss"
Name: "types\web\ext_less"; Description: ".less"
Name: "types\web\ext_jsx"; Description: ".jsx"
Name: "types\web\ext_tsx"; Description: ".tsx"
Name: "types\web\ext_mjs"; Description: ".mjs"
Name: "types\web\ext_vue"; Description: ".vue"
Name: "types\pages"; Description: "{cm:FileGroup_pages}"; Flags: unchecked
Name: "types\pages\ext_html"; Description: ".html"; Flags: unchecked
Name: "types\pages\ext_htm"; Description: ".htm"; Flags: unchecked
Name: "types\pages\ext_svg"; Description: ".svg"; Flags: unchecked
Name: "types\pages\ext_ts"; Description: ".ts"; Flags: unchecked
Name: "types\scripts"; Description: "{cm:FileGroup_scripts}"; Flags: unchecked
Name: "types\scripts\ext_bat"; Description: ".bat"; Flags: unchecked
Name: "types\scripts\ext_cmd"; Description: ".cmd"; Flags: unchecked
Name: "types\scripts\ext_ps1"; Description: ".ps1"; Flags: unchecked
Name: "types\scripts\ext_psm1"; Description: ".psm1"; Flags: unchecked
Name: "types\scripts\ext_vbs"; Description: ".vbs"; Flags: unchecked
Name: "types\scripts\ext_js"; Description: ".js"; Flags: unchecked
Name: "types\scripts\ext_py"; Description: ".py"; Flags: unchecked
Name: "types\scripts\ext_pyw"; Description: ".pyw"; Flags: unchecked
Name: "types\scripts\ext_rb"; Description: ".rb"; Flags: unchecked
Name: "types\scripts\ext_pl"; Description: ".pl"; Flags: unchecked
Name: "types\scripts\ext_sh"; Description: ".sh"; Flags: unchecked
Name: "types\scripts\ext_reg"; Description: ".reg"; Flags: unchecked

[Code]
function ChosenFileTypes(Param: String): String;
begin
  Result := '';
  if WizardIsTaskSelected('types\text\ext_txt') then Result := Result + ' txt';
  if WizardIsTaskSelected('types\text\ext_text') then Result := Result + ' text';
  if WizardIsTaskSelected('types\text\ext_log') then Result := Result + ' log';
  if WizardIsTaskSelected('types\text\ext_nfo') then Result := Result + ' nfo';
  if WizardIsTaskSelected('types\text\ext_md') then Result := Result + ' md';
  if WizardIsTaskSelected('types\text\ext_markdown') then Result := Result + ' markdown';
  if WizardIsTaskSelected('types\text\ext_rst') then Result := Result + ' rst';
  if WizardIsTaskSelected('types\text\ext_adoc') then Result := Result + ' adoc';
  if WizardIsTaskSelected('types\text\ext_tex') then Result := Result + ' tex';
  if WizardIsTaskSelected('types\config\ext_ini') then Result := Result + ' ini';
  if WizardIsTaskSelected('types\config\ext_cfg') then Result := Result + ' cfg';
  if WizardIsTaskSelected('types\config\ext_conf') then Result := Result + ' conf';
  if WizardIsTaskSelected('types\config\ext_config') then Result := Result + ' config';
  if WizardIsTaskSelected('types\config\ext_toml') then Result := Result + ' toml';
  if WizardIsTaskSelected('types\config\ext_yaml') then Result := Result + ' yaml';
  if WizardIsTaskSelected('types\config\ext_yml') then Result := Result + ' yml';
  if WizardIsTaskSelected('types\config\ext_json') then Result := Result + ' json';
  if WizardIsTaskSelected('types\config\ext_jsonc') then Result := Result + ' jsonc';
  if WizardIsTaskSelected('types\config\ext_properties') then Result := Result + ' properties';
  if WizardIsTaskSelected('types\config\ext_editorconfig') then Result := Result + ' editorconfig';
  if WizardIsTaskSelected('types\config\ext_gitignore') then Result := Result + ' gitignore';
  if WizardIsTaskSelected('types\config\ext_gitattributes') then Result := Result + ' gitattributes';
  if WizardIsTaskSelected('types\config\ext_env') then Result := Result + ' env';
  if WizardIsTaskSelected('types\data\ext_csv') then Result := Result + ' csv';
  if WizardIsTaskSelected('types\data\ext_tsv') then Result := Result + ' tsv';
  if WizardIsTaskSelected('types\data\ext_xml') then Result := Result + ' xml';
  if WizardIsTaskSelected('types\data\ext_xsd') then Result := Result + ' xsd';
  if WizardIsTaskSelected('types\data\ext_xsl') then Result := Result + ' xsl';
  if WizardIsTaskSelected('types\data\ext_sql') then Result := Result + ' sql';
  if WizardIsTaskSelected('types\data\ext_diff') then Result := Result + ' diff';
  if WizardIsTaskSelected('types\data\ext_patch') then Result := Result + ' patch';
  if WizardIsTaskSelected('types\code\ext_c') then Result := Result + ' c';
  if WizardIsTaskSelected('types\code\ext_h') then Result := Result + ' h';
  if WizardIsTaskSelected('types\code\ext_cpp') then Result := Result + ' cpp';
  if WizardIsTaskSelected('types\code\ext_cc') then Result := Result + ' cc';
  if WizardIsTaskSelected('types\code\ext_cxx') then Result := Result + ' cxx';
  if WizardIsTaskSelected('types\code\ext_hpp') then Result := Result + ' hpp';
  if WizardIsTaskSelected('types\code\ext_hh') then Result := Result + ' hh';
  if WizardIsTaskSelected('types\code\ext_cs') then Result := Result + ' cs';
  if WizardIsTaskSelected('types\code\ext_java') then Result := Result + ' java';
  if WizardIsTaskSelected('types\code\ext_kt') then Result := Result + ' kt';
  if WizardIsTaskSelected('types\code\ext_kts') then Result := Result + ' kts';
  if WizardIsTaskSelected('types\code\ext_go') then Result := Result + ' go';
  if WizardIsTaskSelected('types\code\ext_rs') then Result := Result + ' rs';
  if WizardIsTaskSelected('types\code\ext_swift') then Result := Result + ' swift';
  if WizardIsTaskSelected('types\code\ext_php') then Result := Result + ' php';
  if WizardIsTaskSelected('types\code\ext_lua') then Result := Result + ' lua';
  if WizardIsTaskSelected('types\code\ext_dart') then Result := Result + ' dart';
  if WizardIsTaskSelected('types\code\ext_scala') then Result := Result + ' scala';
  if WizardIsTaskSelected('types\code\ext_groovy') then Result := Result + ' groovy';
  if WizardIsTaskSelected('types\code\ext_gradle') then Result := Result + ' gradle';
  if WizardIsTaskSelected('types\code\ext_cmake') then Result := Result + ' cmake';
  if WizardIsTaskSelected('types\code\ext_asm') then Result := Result + ' asm';
  if WizardIsTaskSelected('types\code\ext_pas') then Result := Result + ' pas';
  if WizardIsTaskSelected('types\code\ext_fs') then Result := Result + ' fs';
  if WizardIsTaskSelected('types\code\ext_hs') then Result := Result + ' hs';
  if WizardIsTaskSelected('types\code\ext_ex') then Result := Result + ' ex';
  if WizardIsTaskSelected('types\code\ext_exs') then Result := Result + ' exs';
  if WizardIsTaskSelected('types\code\ext_erl') then Result := Result + ' erl';
  if WizardIsTaskSelected('types\code\ext_zig') then Result := Result + ' zig';
  if WizardIsTaskSelected('types\code\ext_jl') then Result := Result + ' jl';
  if WizardIsTaskSelected('types\code\ext_r') then Result := Result + ' r';
  if WizardIsTaskSelected('types\web\ext_css') then Result := Result + ' css';
  if WizardIsTaskSelected('types\web\ext_scss') then Result := Result + ' scss';
  if WizardIsTaskSelected('types\web\ext_less') then Result := Result + ' less';
  if WizardIsTaskSelected('types\web\ext_jsx') then Result := Result + ' jsx';
  if WizardIsTaskSelected('types\web\ext_tsx') then Result := Result + ' tsx';
  if WizardIsTaskSelected('types\web\ext_mjs') then Result := Result + ' mjs';
  if WizardIsTaskSelected('types\web\ext_vue') then Result := Result + ' vue';
  if WizardIsTaskSelected('types\pages\ext_html') then Result := Result + ' html';
  if WizardIsTaskSelected('types\pages\ext_htm') then Result := Result + ' htm';
  if WizardIsTaskSelected('types\pages\ext_svg') then Result := Result + ' svg';
  if WizardIsTaskSelected('types\pages\ext_ts') then Result := Result + ' ts';
  if WizardIsTaskSelected('types\scripts\ext_bat') then Result := Result + ' bat';
  if WizardIsTaskSelected('types\scripts\ext_cmd') then Result := Result + ' cmd';
  if WizardIsTaskSelected('types\scripts\ext_ps1') then Result := Result + ' ps1';
  if WizardIsTaskSelected('types\scripts\ext_psm1') then Result := Result + ' psm1';
  if WizardIsTaskSelected('types\scripts\ext_vbs') then Result := Result + ' vbs';
  if WizardIsTaskSelected('types\scripts\ext_js') then Result := Result + ' js';
  if WizardIsTaskSelected('types\scripts\ext_py') then Result := Result + ' py';
  if WizardIsTaskSelected('types\scripts\ext_pyw') then Result := Result + ' pyw';
  if WizardIsTaskSelected('types\scripts\ext_rb') then Result := Result + ' rb';
  if WizardIsTaskSelected('types\scripts\ext_pl') then Result := Result + ' pl';
  if WizardIsTaskSelected('types\scripts\ext_sh') then Result := Result + ' sh';
  if WizardIsTaskSelected('types\scripts\ext_reg') then Result := Result + ' reg';
end;
