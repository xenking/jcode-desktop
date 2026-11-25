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

Тело `Panel`, его конструктор, применение событий, строки транскрипта и слэш-команды.
Их вызывают дети (`Panel::new`, `apply`, `render_item`, `handle_slash_command`,
`load_history`), поэтому вынести их в соседей нельзя без обратной связи. Попытка
вынести в `events`, `rows`, `construct`, `slash`, `draft`, `history_load`, `scroll`
вернула цикл на 69 модулей, и методы возвращены в корень.
Чтобы довести корень до 2000 строк, нужно сначала разрезать эти методы так, чтобы
дети перестали их вызывать (например, `render_item` через список рендереров,
`new` через построитель), и только потом выносить.
