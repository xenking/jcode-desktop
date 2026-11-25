# Архитектурный и перформанс-аудит Desktop против практик Zed/GPUI, 25 ноября 2026

## Область и метод

- Desktop: `main` на `9c0a6acc`, 252k строк Rust, из них `jcode-desktop-ui` около 130k.
- Референс: Zed `main` на `2664a0aa` (`crates/gpui/examples`, `crates/gpui/src/_accessibility.rs`,
  `crates/gpui/docs/key_dispatch.md`, `docs/contexts.md`, `crates/workspace`, `crates/ui`,
  `crates/markdown`, `assets/keymaps/default-macos.json`).
- Фактический движок Desktop: форк `1jehuang/gpui` (gpui-fast) на `be875c5`, импорт Zed `7960b2a`,
  с retained mode (`docs/retained-mode.md` форка).
- Формат как у improve-codebase-architecture: кандидаты на углубление модулей (deep module,
  seam, locality, leverage) плюс перф-находки. Каждая находка опирается на код. Что не проверено
  исполнением, помечено отдельно в конце.

## Главный контекст: в gpui-fast гранулярность view и есть производительность

gpui-fast переиспользует прошлый кадр для каждого view, пока не изменилось то, что view прочитал
(entities, globals, `ListState`, `ScrollHandle`). Документ форка прямо говорит: чем больше окно
разбито на view, тем дешевле изменение. Обратная сторона: всё, что render читает мимо entities и
globals (`Rc<RefCell<..>>`, статики, время, `canvas` + `Cell`), невидимо для отслеживания и требует
ручного `notify`/`refresh`, а `window.refresh()` и активная accessibility выключают retention на кадр.

Desktop устроен наоборот: несколько огромных view (`Workspace`, `Panel`, `PromptInput`) и много
состояния в статиках и `RefCell`. Поэтому большая часть находок ниже одновременно архитектурная и
перформансная.

## Метрики

| Показатель | Значение | Комментарий |
| --- | ---: | --- |
| Поля `Panel` (`panel.rs:755-1013`) | 177 | `impl Panel` разнесён по 72 файлам |
| Поля `Workspace` (`workspace.rs:795-991`) | 132 | `impl Workspace` в 54 файлах |
| Поля `PromptInput` (`input.rs:212-342`) | 88 | |
| Проверки вида панели (`is_changelog()`, `self.terminal.is_some()`, ...) | 87 | 13 видов панели в одной структуре |
| `impl Render` (без тестов) | ~50 | ~15 из них отдельные tooltip-структуры |
| Сырой матчинг `event.keystroke.key == ...` | ~40 мест | плюс 10 `capture_key_down` и `intercept_keystrokes` |
| `.key_context(...)` | 11 | у Zed десятки контекстов, у Desktop нет `Workspace`/`menu` |
| `KeyBinding` с `None` контекстом | 99 | 85 с контекстом, из них 69 `PromptInput` |
| Accessibility (`.role`, `aria_*`, `on_a11y_action`) | 22 в 8 файлах | против 245 `on_click` и 164 `on_mouse_down` |
| Ad hoc `fn *pill*/*button*/*chip*` | 131 | `#[derive(IntoElement)]` компонентов: 6 |
| `Theme::global()` (статический atomic) | 980 вызовов | не GPUI global, не отслеживается retained mode |
| `EntityInputHandler` реализаций | 4 | `PromptInput`, `LoginInput`, `CodeEditor`, `TerminalPanel` |
| `#[gpui::test]` | 1356 | сильная сторона, как в Zed |

## Что уже сделано хорошо

- Виртуализация основных списков (`gpui::list` для транскрипта и сайдбара, `uniform_list` для файлов).
- Слои view ацикличны: `panel` и `input` не импортируют `workspace` вне тестов, команды идут через
  `ui_actions.rs`. Это прямой аналог `window.dispatch_action` в Zed.
- Сильная измерительная инфраструктура: `render_stats`, live profile, `PERFORMANCE.md`,
  `forbid_blocking_local_io()` в render.
- Уже выделены листовые view там, где частота высокая: `pulse_text::Tickers`, `activity::Spinner`,
  `SidebarChrome`, metadata-entity сайдбара.
- Тесты идут через настоящий keymap (`bind_workspace_keys`) и `TestAppContext`, как в
  `examples/testing.rs` Zed.

## Кандидаты на углубление

Порядок по соотношению выгоды и риска. Первые три дают основной перф-эффект.

### 1. Markdown как сущность с кэшем разбора, живая строка как отдельный view

**Файлы:** `markdown.rs:2778-2807`, `markdown.rs:46-50`, `markdown.rs:1434-1450`,
`panel.rs:5252-5262`, `panel.rs:7795-7840`.

**Проблема.** `render_document_with_prompt_background` вызывает `parse(source)` на каждый render
каждой видимой строки. Во время стрима `text_reveal.tick` запрашивает animation frame каждый кадр
(`panel.rs:5261`), значит `Panel` перестраивается с частотой дисплея и повторно разбирает markdown
всех видимых строк. Собственный профиль в `PERFORMANCE.md` (2026-11-15) показывает, что построение
markdown-документа занимает примерно половину inclusive samples в panel render. Параметры рендера
передаются через `thread_local!` (`STREAM_FADE`, `LEAF_FADE`, `SEARCH_TERMS`, `TRANSCRIPT_LINKS`,
`FILE_OPTIONS`), это скрытая связность: корректность зависит от порядка вызовов.

**Как в Zed.** `crates/markdown/src/markdown.rs:496`: `Markdown` это entity с `append()`, разбором
в фоне (`pending_parse`, `should_reparse`, `start_background_parse`) и готовым деревом для
`MarkdownElement`. Стрим дописывает в entity, notify получает только она.

**Решение.** Модуль `TranscriptDocument` с маленьким интерфейсом: `set(source)`, `append(delta)`,
`element(style)`. Внутри: кэш `Arc<[Block]>` по ревизии элемента транскрипта, инкрементальный
разбор хвоста для стрима (переразбирать только последний незакрытый блок), стиль и fade как явный
аргумент вместо thread-local. Живую строку стрима вынести в отдельный view, чтобы reveal-анимация
нотифицировала только его, а `Panel` и устоявшиеся строки переигрывались из прошлого кадра.

**Выгода.** Перф: уходит повторный разбор на каждом кадре стрима. Locality: весь markdown-путь в
одном модуле с одним тестовым швом. Модель разбора без GPUI можно унести в `jcode-desktop-model`.

**Проверка.** Существующий `profile-render.py` со сценарием стрима: доля markdown в samples, draw
time p95, `render_stats` счётчик Panel renders/s до и после.

### 2. `Panel` как 13 видов в одной структуре, нужен Item-подобный шов

**Файлы:** `panel.rs:755-1013` (поля), `panel.rs:7203-7400` (render как цепочка `if`),
72 файла `impl Panel`.

**Проблема.** Одна сущность одновременно чат, терминал, просмотр файла, side document, changelog,
accounts, change review, applet, Todoist, OpenRig, orchestration, unfinished work и login. Состояние
каждого вида это `Option`-поле, render выбирает ветку по порядку проверок, а 87 проверок вида
разбросаны по модулям. Поля чата (транскрипт, стрим, picker'ы, swarm, ask, очередь) живут и в
терминальной панели. Любой notify перестраивает всю структуру, retained mode не может разделить
виды, тесты вида поднимают весь `Panel`.

**Как в Zed.** `crates/workspace/src/item.rs:170`: `trait Item: Focusable + EventEmitter + Render`,
каждый вид (Editor, TerminalView, ProjectSearchView) отдельная entity, `Pane` хранит
`Box<dyn ItemHandle>`. Док-панели через `trait Panel` (`dock.rs:36`).

**Решение.** `Slot` хранит `Entity<dyn PanelItem>`-подобный handle (через `AnyView` плюс трейт
`PanelItem { title, focus_handle, snapshot, supports_voice, ... }`). Начать с самых независимых
видов: терминал, файл, changelog, Todoist, OpenRig. Чат остаётся `ChatPanel`. Общий хром (заголовок,
фокус-рамка, закрытие) живёт в слоте, а не в каждом виде. Hot-reload snapshot становится
per-kind (`PanelSnapshot` как enum по видам).

**Выгода.** Leverage: Workspace работает с одним интерфейсом вместо 87 проверок. Locality: вид
меняется в своём файле. Перф: нотификация терминала или OpenRig не трогает дерево чата.
Тесты вида без поднятия чата.

**Риск.** Крупный рефактор, много конфликтов с параллельными сессиями. Делать по одному виду за
коммит, начиная с терминала (там уже есть `Entity<TerminalPanel>`, `panel.rs:7282`).

### 3. Workspace как медиатор каждого события сессии

**Файлы:** `workspace.rs:1028-1057` (bridge task), `workspace.rs:2466-3320` (`apply`),
`workspace.rs:2880-2988`, `workspace.rs:9823-9848` (`SidebarChrome`).

**Проблема.** Каждое `Update::Event`, включая каждую текстовую дельту стрима, проходит через
`Workspace::apply`, которая в конце возвращает `true` (`workspace.rs:3319`), и bridge task делает
`cx.notify()` на Workspace. Кроме того, каждое событие пишет `session.updated_at_ms = now`
(`workspace.rs:2937`), меняя порядок каталога. `SidebarChrome` читает весь `Workspace` через
`update`, поэтому перестраивается на каждый батч событий. `PERFORMANCE.md` фиксирует около 30
построений сайдбара в секунду при стриме, это и есть каденс bridge. Также `Update::CatalogTodos`
безусловно нотифицирует все панели (`workspace.rs:2588`), хотя `set_global(CatalogTasks)` рядом
уже инвалидирует только читателей.

**Как в Zed.** Модели (`Project`, `Buffer`, `Thread`) отдельные entities. View подписываются на
них через `cx.observe`/`cx.subscribe` (`_ownership_and_data_flow.rs`), Workspace не ретранслирует
поток данных.

**Решение.** Модуль `SessionStore` (entity) владеет каталогом сессий, worker meta, swarm status,
todo progress. Каждая сессия получает модель `Entity<Session>`, bridge маршрутизирует событие прямо
в неё, `ChatPanel` наблюдает свою сессию. Workspace подписывается только на структурные события
(новая сессия, фокус, закрытие). Recency в каталоге обновлять с округлением (например до секунды)
или по `TurnDone`/`ToolDone`, а не на каждую дельту. Сайдбар читает `SessionStore`, а не Workspace.

**Выгода.** Перф: стрим не трогает Workspace и сайдбар. Locality: логика каталога из 18k-строчного
`workspace.rs` уходит в модуль с узким интерфейсом. Модель каталога без GPUI тестируется в
`jcode-desktop-harness`/`-model`.

### 4. Key dispatch: сырые keystroke вместо actions, контекстов и keymap как данных

**Файлы:** `lib.rs:134-...` (`bind_workspace_keys`), `lib.rs:353`, `workspace.rs:1001`,
`workspace.rs:10147-10220`, ~40 мест `event.keystroke.key == "escape"|"enter"` (`panel.rs:7259`,
`panel.rs:7274`, `panel.rs:8117-8134`, `live_tabs.rs:682`, `workspace_menu.rs:50`, `queue_card.rs:41`,
`panel_ask.rs:732`, `panel_openrig.rs:774`, ...).

**Проблема.**
- Escape, Enter, стрелки в меню и карточках обрабатываются сырыми `on_key_down`. Их нельзя
  переназначить, они не видны в подсказках клавиш и меню, а `capture_key_down` на корне Workspace
  (`workspace.rs:10166-10207`) перехватывает клавиши раньше фокусного элемента, включая IME.
- Нет контекстов `Workspace`, `menu`, `Picker`. 99 привязок с `None` контекстом.
- **Утечка привязок между поколениями hot reload.** `activate` вызывает `bind_workspace_keys(app)`
  на каждой активации поколения (`lib.rs:353`), а `Workspace::new` вызывает `input::bind_keys` на
  каждое окно (`workspace.rs:1001`). `Keymap::add_bindings` не дедуплицирует (gpui-fast
  `keymap.rs:63`), `clear_key_bindings` не вызывается нигде. Keymap растёт примерно на 250 привязок
  за reload, старые привязки указывают на типы actions из выгруженного по смыслу поколения.
  Комментарий в `workspace.rs:10212-10214` прямо описывает симптом: старые привязки hot reload
  выполняют то же движение повторно. Так же копятся глобальные `app.on_action(Quit)` из каждого
  поколения (`lib.rs:354`).

**Как в Zed.** `key_dispatch.md`: функциональность для клавиатуры это action в key context.
Пикеры и меню используют общие `menu::Confirm/Cancel/SelectNext/SelectPrevious` в контексте `menu`
(`default-macos.json:48`), workspace в `Workspace` (`:696`), терминал в `Terminal` (`:1378`).
Keymap это JSON-данные (136 контекстных блоков), пользовательский keymap накладывается сверху.

**Решение.**
1. Сразу: на активации поколения `cx.clear_key_bindings()` и одна полная перерегистрация. Убрать
   `bind_keys` из `Workspace::new`. Глобальные `on_action` регистрировать один раз на поколение с
   идемпотентной заменой.
2. Ввести `menu`-actions и контексты `Workspace`, `Menu`, `Picker`, `Card`. Заменить сырой матчинг
   на `.on_action(cx.listener(...))`.
3. Описать keymap данными (таблица или JSON в `assets`), с одним загрузчиком для приложения и
   тестов и возможностью пользовательских переопределений. Подсказки и меню строить из keymap.

**Проверка.** Тест: N активаций подряд не увеличивают `cx.key_bindings().borrow().bindings().len()`.
Существующие keyboard-тесты из `KEYBOARD_AUDIT.md` должны пройти без изменений поведения.

### 5. Режимы Workspace как булевы флаги, нужен ModalLayer

**Файлы:** `workspace.rs:9851-9900` (ветки render), `workspace.rs:10167-10207` (охранные условия),
`panel.rs:1002-1004`, `panel.rs:984`, `panel.rs:7207-7228`.

**Проблема.** Онбординг-ошибка, sign-in, resume, single panel, rename, folder picker, usage reset,
beta notice, compact sidebar, overview закодированы булевыми полями и `Option`. Корневой обработчик
клавиш проверяет восемь условий подряд. В `Panel` popover-фокус восстанавливается вручную через
`ask_focus_return`, `composer_popover_watch`, `composer_popover_trigger`, `focus_pending`, и
применяется внутри render.

**Как в Zed.** `workspace/src/modal_layer.rs`: `ModalView: ManagedView`, `toggle_modal::<V>()`,
закрытие через `DismissEvent`, `on_before_dismiss`, предыдущий фокус восстанавливается слоем.
Popover через `PopoverMenu`/`ContextMenu` из `crates/ui`, пример `examples/popover.rs`
(`deferred` + `anchored`).

**Решение.** `ModalLayer`-entity в Workspace и `PopoverLayer`-хелпер для панели. Каждый режим
это отдельный view со своим key context и focus handle, Escape это `menu::Cancel` внутри него.
Охранные условия корня исчезают, потому что dispatch уже ограничен фокусом.

**Выгода.** Locality: каждый режим целиком в своём модуле. Меньше логики фокуса в render.

### 6. Одно текстовое ядро вместо трёх редакторов

**Файлы:** `input.rs` (8077 строк, `EntityInputHandler` на `:4120`, `TextElement` на `:4337`),
`jcode-desktop-accounts-ui/src/login_input.rs:201,355`, `code_editor.rs:751,918`,
`jcode-desktop-model/src/text_buffer.rs`.

**Проблема.** Три независимые реализации курсора, выделения, IME, словной навигации и text
element. Только `CodeEditor` использует GPUI-free `TextBuffer`. Баги IME и выделения приходится
чинить в трёх местах.

**Как в Zed.** Один `Editor` в режимах single line, auto height, full. Пример
`examples/view_example`: `Editor` как рабочая entity (курсор, blink, фокус, клавиатура,
рендерер), `Input`/`TextArea` как тонкие слои формы над ней.

**Решение.** `TextBuffer` из `jcode-desktop-model` как единственная модель правки и выделения,
один `TextField` element и `EntityInputHandler`, режимы single line (login), composer (prompt с
вложениями и pickers поверх), code (подсветка). `PromptInput` становится оболочкой над ядром.

**Выгода.** Leverage: одно исправление IME или навигации сразу везде. Тесты правки без GPUI в
model-крейте, где они компилируются за секунду.

### 7. Слой UI-компонентов как `crates/ui` в Zed

**Файлы:** 131 функция `*pill*/*button*/*chip*` (больше всего в `input.rs`, `workspace.rs`,
`panel_voice_pills.rs`), tooltip-структуры `TabTooltip`, `LocationTooltip`, `EffortTooltip`,
`CacheTooltip`, `StatsTooltip`, `RowsTooltip`, `MeterTooltip`, `LimitsTooltip`, `HeaderTooltip`,
`SpeedTooltip`, `JobTooltip`, `BuildTooltip`, `WorkersTooltip`, `VoiceTooltip`, ...

**Проблема.** Каждый экран заново собирает pill из `div()` с темой, hover, focus ring и
обработчиком. Правила `AGENTS.md` о pill-форме и фокусе исполняются копированием, а не типом.
Accessibility и подсказки клавиш нельзя добавить в одном месте.

**Как в Zed.** `crates/ui/src/components`: `Button`, `IconButton`, `Toggle`, `Disclosure`,
`ListItem`, `Tooltip` (`Tooltip::text`, `Tooltip::for_action`), `ContextMenu`, `PopoverMenu`,
`KeyBinding`. Это `RenderOnce`-компоненты, в которые уже встроены role и aria
(`toggle.rs:484`, `disclosure.rs:103`, `tree_view_item.rs:163`).

**Решение.** Модуль компонентов в `jcode-desktop-ui-core` (он уже GPUI-крейт и быстро
собирается): `Pill` (primary, secondary, compact), `IconPill`, `PillRow`, `Tooltip` (текст,
текст плюс action, кастомный builder), `Disclosure`, `MenuList`. Встроить focus ring, `on_click`,
role и aria label, keybinding hint. Переводить экраны постепенно.

**Выгода.** Leverage для дизайн-правил и accessibility. Меньше кода в 130k-строчном крейте.

### 8. Accessibility практически отсутствует

**Факты.** 22 использования `role`/`aria_*` в 8 файлах против 245 `on_click` и 164
`on_mouse_down`. `on_mouse_down` не регистрирует accessible `Click`, `on_click` регистрирует
(`_accessibility.rs`, раздел Handling actions). Форк уже содержит AccessKit (`window/a11y.rs`).

**Как в Zed.** `_accessibility.rs`: узел попадает в дерево при наличии `GlobalElementId` и role,
стабильные ID между кадрами важны, потому что смена ID читается скринридером как удаление и
вставка. Для текста `text!` с уникальным ID в циклах. Для своих элементов (редактор)
`a11y_synthetic_children`. Пример `examples/a11y.rs`.

**Решение.** Делать через компоненты из пункта 7, а не точечно. Затем: role для строк
транскрипта и сайдбара, `Role::TextInput` с synthetic text run для composer, live region для
стрима. Перевести кнопочные `on_mouse_down` на `on_click`, где нет drag-семантики.

**Перф-оговорка.** В gpui-fast retained mode выключается, пока accessibility активна. С
VoiceOver стоимость кадра вернётся к уровню upstream. Это ещё один аргумент за пункты 1 и 3,
которые снижают стоимость полного render.

### 9. Чистота render

**Файлы:** `panel.rs:7207-7243`, `workspace.rs:9855-9870`, `workspace.rs:9893-9912`,
`sidebar_projects.rs:63-82,193-240`, `markdown.rs:639` плюс
`jcode-desktop-model/src/markdown_file.rs:66`.

**Проблема.** Render меняет фокус (`focus_input`, `window.focus`), сэмплирует анимации и пишет
результаты в поля, запускает проверку обновлений (`updates::ensure_release_check()`), создаёт
entity метаданных, читает файловую систему на главном потоке: probe `.git` для сайдбара с TTL и
`path.is_file()` для каждого токена транскрипта, похожего на путь. Это тот самый класс зависаний,
который ловит новый macOS watchdog (`6657e68e`).

**Как в Zed.** Render строит элементы из состояния. Фокус меняется в обработчиках actions,
анимации через `with_animation`/`request_animation_frame` без записи в модель, файловая система
читается фоновым worktree scanner и в render доступна как снимок в памяти.

**Решение.** Перенести фокус в обработчики событий, `ensure_release_check` в инициализацию и таймер.
Git probe и существование файлов делать в фоновой задаче с кэшем в entity, render читает только
кэш, промах кэша планирует проверку и рисует ссылку без подтверждения.

**Статус (файловая система).** Сделано: `jcode_desktop_model::probe_cache::ProbeCache` хранит ответы
в памяти, промах или устаревшее значение ставит ключ в очередь, фоновая задача
(`background_probe::run`) проверяет диск и перерисовывает окно, если ответ изменился. Так работают
упоминания файлов в транскрипте, git-состояние сайдбара (`.git`, HEAD, commondir), аккаунт в сайдбаре
и проверка SuperWhisper. `fs_probe::count()` считает проверки на главном потоке, тест
`render_frames_probe_filesystem_counts` держит их на нуле. Остальное из пункта 9 (фокус и анимации в
render, `ensure_release_check`) не тронуто.

### 10. Состояние, невидимое для retained mode

**Файлы:** `theme.rs:144,519` (`Theme::global()` на atomic), `config.rs:212,275`
(`CONFIG`, `DISPLAY`), `panel.rs:841,854,873,878,958,961` (`RefCell`-поля, часть пишется во
время render), `workspace` `file_tree`, `folder_candidates` в `RefCell`, 40 `canvas(...)` для
измерений с `Rc<Cell<Bounds>>`.

**Проблема.** gpui-fast отслеживает чтения entities и globals. Тема, конфиг, `RefCell` и
измерения через `canvas` + `Cell` идут мимо, поэтому код вынужден вызывать `refresh_windows()` или
ручные notify, а полный refresh выключает retention на кадр. Измерение в paint и использование на
следующем кадре даёт лишний кадр и мигание. `swarm_row_focus` и `swarm_scrolls` никогда не
очищаются. `config::display()` клонирует `DisplayConfig` под `RwLock` на каждую строку списка
(`panel.rs:7839`).

**Как в Zed.** Тема и настройки это `Global` (`cx.theme()`, `SettingsStore`), изменения через
`observe_global` и точечные notify. Состояние элементов через `window.use_keyed_state` и
element state, не через `RefCell` во view.

**Решение.** `ThemeGlobal` и `DisplayGlobal` как GPUI globals, читать через `cx`. Тогда смена
темы инвалидирует ровно читателей без `refresh_windows`. Per-row focus и scroll handles через
`use_keyed_state`. Для измерений использовать prepaint-хуки или element state.

### 11. Асинхронность и фоновые потоки

**Файлы:** `workspace_usage_reset.rs:69-75`, `workspace_account_sign_in.rs:260`,
`sidebar_account_menu.rs:111`, `panel_voice.rs:483,1072`, `panel_special_commands.rs:195`,
65 `std::thread::spawn`.

**Проблема.** Для сетевых вызовов создаётся новый Tokio current-thread runtime и блокируется
поток фонового пула GPUI на всё время запроса. Пул ограничен, параллельные долгие операции
(голос, usage reset, аккаунты) могут вытеснить короткие фоновые задачи.

**Как в Zed.** Один HTTP-клиент на процесс (`http_client`/`reqwest_client` с общим runtime),
GPUI-задачи только ждут future.

**Решение.** Общий runtime handle (один на процесс, у host или в harness) и функция
`spawn_network(future) -> Task<T>`, которая не блокирует поток пула.

### 12. Hot reload: двойная статическая линковка GPUI

**Файлы:** `src/host/reload.rs`, `crates/jcode-desktop-api`, `docs/hot-reload-memory.md`.

**Проблема.** Host и каждый UI cdylib статически линкуют GPUI, поэтому любой thread-local или
static в GPUI существует дважды. Утечка element arena на ~13 GiB (`hot-reload-memory.md`) была
именно этим классом ошибки. Каждое следующее изменение gpui-fast, добавляющее TLS, рискует тем же.
Утечка keymap из пункта 4 того же рода: состояние App переживает поколение, а регистрации копятся.

**Как в Zed.** Upstream GPUI теперь имеет `hot_reload.rs` на subsecond (`dx serve --hot-patch`):
патч применяется между foreground-обновлениями, затем `refresh_windows()`. Один экземпляр GPUI,
без отдельного ABI.

**Решение.** Краткосрочно: инвентаризация всего, что поколение регистрирует в долгоживущем `App`
(keymap, global actions, observers, globals), и явная очистка на деактивации. Тест на N активаций.
Долгосрочно: оценить subsecond-путь после того, как форк догонит upstream. Это снимает ABI
`HostApi`, класс ошибок с двойными TLS и ограничение "один 130k-строчный cdylib", которое сейчас
мешает дробить UI на крейты, как в Zed.

## Быстрые победы

| Изменение | Файл | Риск |
| --- | --- | --- |
| `clear_key_bindings()` и одна регистрация на активацию, убрать `bind_keys` из `Workspace::new` | `lib.rs:353`, `workspace.rs:1001` | низкий |
| `CatalogTodos` без безусловного notify всех панелей | `workspace.rs:2588` | низкий |
| Recency каталога не на каждую дельту стрима | `workspace.rs:2937` | низкий |
| Очищать `swarm_row_focus`, `swarm_scrolls` по текущим worker | `panel.rs:841,878` | низкий |
| `config::display()` один раз на render панели, не на строку | `panel.rs:7839` | низкий |
| `format!("{payload:?}")` в хэше заменить на `Hash` | `panel.rs:497` | низкий |
| Кнопочные `on_mouse_down` перевести на `on_click` | 164 места | средний, проверить drag |

## Дорожная карта

1. **Фаза 0, неделя.** Быстрые победы. Тест на стабильный размер keymap после N активаций.
   Базовый замер стрима: Panel renders/s, sidebar builds/s, draw p95, доля markdown.
2. **Фаза 1, перф.** Пункт 1 (markdown entity и живая строка) и пункт 3 (`SessionStore`,
   `Entity<Session>`). Повторный замер тем же сценарием.
3. **Фаза 2, швы.** Пункт 2 по одному виду панели, начиная с терминала. Пункт 5 (ModalLayer).
   Пункт 4 (actions, контексты, keymap как данные).
4. **Фаза 3, фундамент.** Пункт 7 (компоненты), на нём пункт 8 (accessibility), пункт 6
   (текстовое ядро), пункт 10 (globals вместо статиков).
5. **Фаза 4, платформа.** Пункты 11 и 12.

## Не проверено исполнением

- Рост keymap после reload выведен из кода (`add_bindings` без дедупликации, нет вызовов
  `clear_key_bindings`) и комментария в `workspace.rs:10212`. Число привязок в живом host не
  измерялось.
- Доля markdown в кадре взята из профиля `PERFORMANCE.md` от 2026-11-15, новый профиль в рамках
  аудита не снимался.
- Перестройка Panel на каждом кадре стрима выведена из `request_animation_frame` в
  `panel.rs:5261` и семантики retained mode, счётчики `render_stats` не снимались.
- Влияние accessibility на retained mode взято из документации форка.

## Как воспроизвести метрики

```sh
rg -c 'impl Render for' --type rust crates
rg -n 'event\.keystroke\.key\s*==' --type rust crates | grep -v _tests
rg -n '\.role\(|aria_|on_a11y_action' --type rust crates | grep -v _tests
rg -n 'clear_key_bindings|bind_workspace_keys\(|input::bind_keys\(' --type rust src crates
rg -n 'fn \w*(pill|button|chip)\w*\(' --type rust crates | grep -v _tests | wc -l
```
