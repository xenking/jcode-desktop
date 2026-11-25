# Panel layering

Продолжение пункта 2 аудита `2026-11-25-gpui-architecture.md`. Цель: убрать
`module-cycle`, `tangle` и `field-leak` у `jcode_desktop_ui::panel`.

## Что давало цикл

`Panel` это одна вещь, чьи методы лежат в 34 файлах. Для deslop метод принадлежит
модулю, где написан `impl`. Поэтому `composer` вызывал `Panel::render_*` из соседей,
а соседи брали у `composer` пиллы и константы. Любой общий виджет или предикат,
лежавший в «своей» фиче, замыкал кольцо.

## Целевые слои

Модуль может использовать только модули ниже себя. Корень `panel.rs` свободно
использует детей и они его, это не цикл.

```
L5  composer, render, todoist, facade          верхний слой, на них никто не ссылается
L4  session_status, decision, ask_images,      состояния и карточки, видят только то,
    swarm_indicator, preview, voice,           что ниже
    side_question(+inline), recovery
L3  ask, queue, login(+accounts,               конечные автоматы Panel
    account_rows, provider_picker),
    history_paging(+rewind), prompt,
    stop_reason, usage, context_usage
L2  image_pane, image_preview, clean_chat,     виды и данные одной фичи
    model_switch, cache_timer(+card)
L1  gallery, items, classify, turn, live       данные и предикаты без UI-зависимостей
L0  chrome, meter, labels, todo_card,          листья: виджеты, текст, константы
    tool_summary, history_items, expandable
```

Правила:

- Лист принимает данные, которые рисует, и не принимает `&Panel`.
  Карточкам, пиллам и подсказкам, которым нужен живой вид панели, панель выдаёт
  `Live<T>` (`panel_live.rs`): вопрос, заданный в момент создания, и подписка на
  изменения. Лист знает ответ, а не `Panel`.
- Всё, что вызывают из нескольких фич, живёт ниже них: `chrome` (пиллы, `BAR_ICON`),
  `meter` (карточки и подсказки метрик), `classify` (предикаты над строками
  транскрипта), `turn` (`TurnOutcome`, `StopNotice`, `AutoRetry`), `gallery`.
- Пары, которые описывают одно понятие, вложены: `cache_timer::card`,
  `side_question::inline`, `history_paging::rewind`, `login::account_rows`.
- Контейнер (workspace) спрашивает панель и меняет её только через `facade`.
  Поля `Panel` наружу не торчат, изменения сами вызывают `cx.notify()`.
- Автоматы (`AutoPoke`, `TailGlide`, `WheelGlide`, `TimedPreview`, `Detector`,
  `Gallery`) принимают решение в своих методах. `Panel` применяет ответ.

## Что остаётся в `panel.rs`

Тело `Panel` (поля), типы состояния и точки входа, которые вызывают дети:
`Panel::new`, `apply`, `render_item`, `handle_slash_command`, `load_history`,
`connect_input`, `snapshot`. Тела этих методов живут в модулях верхнего слоя
(`construct`, `rows`, `events`, `slash`, `history_load`, `draft`, `pickers`, `status`,
`focus`, `scroll`), а в корне остаётся тонкая обёртка. Дети зовут обёртку, обёртка зовёт
тело, поэтому дети не зависят от модулей верхнего слоя и цикла нет.

Попытка вынести методы целиком (без обёрток) вернула цикл на 69 модулей: дети вызывали
`new`, `apply`, `render_item`, `load_history`. Правило: метод, который зовёт хотя бы один
ребёнок, оставляет в корне только сигнатуру.

## Что осталось

`panel::login` (24 из 35), `panel_item` (28 из 47) и `workspace::global_voice` (25 из 26)
остаются «мелкими» модулями: их Panel-методы лежат в дочерних модулях, а для deslop дом
метода это `panel`, поэтому каждый вспомогательный вызов считается интерфейсом. Нужны
свободные функции вместо методов `Panel` внутри `login`, трейт хоста для `global_voice`
и разбиение `PanelItem`.
