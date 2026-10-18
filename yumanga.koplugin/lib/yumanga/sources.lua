-- Source selection: asks the core which sources exist, remembers which ones
-- are enabled, and warns when too many are enabled for a small e-reader.
local ConfirmBox = require("ui/widget/confirmbox")
local InfoMessage = require("ui/widget/infomessage")
local InputDialog = require("ui/widget/inputdialog")
local UIManager = require("ui/uimanager")
local T = require("ffi/util").template
local _ = require("gettext")

local Core = require("lib/yumanga/core")

local Sources = {}

-- Searching runs one source after another, but every extra source still adds
-- results, covers and detail requests. Above this, the user gets a warning.
Sources.MAX_RECOMMENDED = 3

local SETTING = "yumanga_selected_sources"
local SEARCH_SETTING = "yumanga_source_search"
local LANGUAGE_SETTING = "yumanga_source_language"
local RATING_SETTING = "yumanga_source_rating"
local RATINGS = {
	[0] = _("Safe"),
	[1] = _("Suggestive"),
	[2] = _("NSFW"),
}

-- Sources known to the core executable: { { id = "...", name = "..." }, ... }.
-- Cached per session; `refresh` reloads the online Aidoku catalog.
function Sources.available(plugin, refresh)
	if plugin.sources and not refresh then
		return plugin.sources
	end
	local output, succeeded = Core.run(plugin.plugin_directory, "sources")
	local data, err = Core.decodeResponse(output, succeeded)
	if not data then
		plugin.sources_error = err
		return {} -- not cached, so the next call tries again
	end
	plugin.sources_error = nil
	plugin.sources = data
	return data
end

local function isAidoku(source)
	return source.id:match("^[%w%-]+%.") ~= nil
end

function Sources.refresh(plugin, touchmenu_instance)
	plugin.sources = nil
	local available = Sources.available(plugin, true)
	if #available == 0 and plugin.sources_error then
		UIManager:show(InfoMessage:new({
			text = plugin.sources_error,
			timeout = 6,
		}))
		return
	end
	local aidoku_count = 0
	for _i, source in ipairs(available) do
		if isAidoku(source) then
			aidoku_count = aidoku_count + 1
		end
	end
	UIManager:show(InfoMessage:new({
		text = T(_("Loaded %1 sources, including %2 Aidoku sources."), #available, aidoku_count),
		timeout = 4,
	}))
	if touchmenu_instance then
		touchmenu_instance:updateItems()
	end
end

-- Enabled sources, in the core's order. Falls back to the first source when
-- nothing valid is stored (first run, or a stored source no longer exists).
function Sources.selected(plugin)
	local available = Sources.available(plugin)
	local wanted = {}
	local saved = G_reader_settings:readSetting(SETTING)
		or G_reader_settings:readSetting("weebcentral_selected_sources")
		or {}
	for _i, id in ipairs(saved) do
		wanted[id] = true
	end
	local picked = {}
	for _i, source in ipairs(available) do
		if wanted[source.id] then
			picked[#picked + 1] = source
		end
	end
	if #picked == 0 and #available > 0 then
		picked[1] = available[1]
	end
	return picked
end

function Sources.isSelected(plugin, id)
	for _i, source in ipairs(Sources.selected(plugin)) do
		if source.id == id then
			return true
		end
	end
	return false
end

-- "WeebCentral, Other" for dialogs.
function Sources.names(plugin)
	local names = {}
	for _i, source in ipairs(Sources.selected(plugin)) do
		names[#names + 1] = source.name
	end
	return table.concat(names, ", ")
end

local function save(ids)
	G_reader_settings:saveSetting(SETTING, ids)
end

local function openSearchDialog(touchmenu_instance)
	local dialog
	dialog = InputDialog:new({
		title = _("Search sources"),
		input = G_reader_settings:readSetting(SEARCH_SETTING) or "",
		input_hint = _("Source name or ID"),
		buttons = {
			{
				{
					text = _("Cancel"),
					id = "close",
					callback = function()
						UIManager:close(dialog)
					end,
				},
				{
					text = _("Search"),
					is_enter_default = true,
					callback = function()
						local query = dialog:getInputText():match("^%s*(.-)%s*$")
						G_reader_settings:saveSetting(SEARCH_SETTING, query)
						UIManager:close(dialog)
						if touchmenu_instance then
							touchmenu_instance:updateItems()
						end
					end,
				},
			},
		},
	})
	UIManager:show(dialog)
	dialog:onShowKeyboard()
end

local function filterItems(source, query, language, rating)
	local name = (source.name or ""):lower()
	local id = (source.id or ""):lower()
	if query ~= "" and not name:find(query, 1, true) and not id:find(query, 1, true) then
		return false
	end
	if language then
		local matches_language = false
		for _i, candidate in ipairs(source.languages or {}) do
			if candidate == language then
				matches_language = true
				break
			end
		end
		if not matches_language then
			return false
		end
	end
	if rating ~= nil and source.content_rating ~= rating then
		return false
	end
	return true
end

local function languageOptions(available)
	local found, languages = {}, {}
	for _i, source in ipairs(available) do
		for _j, language in ipairs(source.languages or {}) do
			if not found[language] then
				found[language] = true
				languages[#languages + 1] = language
			end
		end
	end
	table.sort(languages)
	local selected = G_reader_settings:readSetting(LANGUAGE_SETTING)
	local items = {
		{
			text = _("All languages"),
			checked_func = function()
				return selected == nil
			end,
			keep_menu_open = true,
			callback = function(touchmenu_instance)
				G_reader_settings:delSetting(LANGUAGE_SETTING)
				if touchmenu_instance then
					touchmenu_instance:updateItems()
				end
			end,
		},
	}
	for _i, language in ipairs(languages) do
		items[#items + 1] = {
			text = language,
			checked_func = function()
				return selected == language
			end,
			keep_menu_open = true,
			callback = function(touchmenu_instance)
				G_reader_settings:saveSetting(LANGUAGE_SETTING, language)
				if touchmenu_instance then
					touchmenu_instance:updateItems()
				end
			end,
		}
	end
	return items
end

local function ratingOptions()
	local selected = G_reader_settings:readSetting(RATING_SETTING)
	local items = {
		{
			text = _("All ratings"),
			checked_func = function()
				return selected == nil
			end,
			keep_menu_open = true,
			callback = function(touchmenu_instance)
				G_reader_settings:delSetting(RATING_SETTING)
				if touchmenu_instance then
					touchmenu_instance:updateItems()
				end
			end,
		},
	}
	for rating = 0, 2 do
		items[#items + 1] = {
			text = RATINGS[rating],
			checked_func = function()
				return selected == rating
			end,
			keep_menu_open = true,
			callback = function(touchmenu_instance)
				G_reader_settings:saveSetting(RATING_SETTING, rating)
				if touchmenu_instance then
					touchmenu_instance:updateItems()
				end
			end,
		}
	end
	return items
end

-- Enables or disables one source. Keeps at least one enabled and asks for
-- confirmation when enabling would pass MAX_RECOMMENDED.
function Sources.toggle(plugin, source, touchmenu_instance)
	local ids, was_selected = {}, false
	for _i, current in ipairs(Sources.selected(plugin)) do
		if current.id == source.id then
			was_selected = true
		else
			ids[#ids + 1] = current.id
		end
	end

	local function refresh()
		if touchmenu_instance then
			touchmenu_instance:updateItems()
		end
	end

	if was_selected then
		if #ids == 0 then
			UIManager:show(InfoMessage:new({
				text = _("At least one source must stay selected."),
				timeout = 3,
			}))
			return
		end
		save(ids)
		refresh()
		return
	end

	ids[#ids + 1] = source.id
	if #ids > Sources.MAX_RECOMMENDED then
		UIManager:show(ConfirmBox:new({
			text = T(
				_(
					"You are about to search %1 sources at once (recommended: %2 or fewer).\n\nE-readers have little memory, so many sources can make searching slow or freeze the device. Turning off \"Show author, status and tags in results\" helps.\n\nUse it anyway?"
				),
				#ids,
				Sources.MAX_RECOMMENDED
			),
			ok_text = _("Use anyway"),
			cancel_text = _("Cancel"),
			ok_callback = function()
				save(ids)
				refresh()
			end,
		}))
		return
	end
	save(ids)
	refresh()
end

-- Menu entries, one checkbox per source. Built every time the submenu opens,
-- so a source added to the core shows up without any change to the UI.
function Sources.menuItems(plugin)
	local available = Sources.available(plugin, true)
	if #available == 0 then
		return { { text = _("No sources found"), enabled = false } }
	end
	local query = (G_reader_settings:readSetting(SEARCH_SETTING) or ""):lower()
	local language = G_reader_settings:readSetting(LANGUAGE_SETTING)
	local rating = G_reader_settings:readSetting(RATING_SETTING)
	local items = {
		{
			text = query == "" and _("Search sources...") or T(_("Search: %1"), query),
			callback = function(touchmenu_instance)
				openSearchDialog(touchmenu_instance)
			end,
		},
		{
			text = language and T(_("Language: %1"), language) or _("Language: all"),
			sub_item_table_func = function()
				return languageOptions(available)
			end,
		},
		{
			text = rating ~= nil and T(_("Content rating: %1"), RATINGS[rating] or tostring(rating))
				or _("Content rating: all"),
			sub_item_table_func = ratingOptions,
		},
		{
			text = _("Clear source filters"),
			callback = function(touchmenu_instance)
				G_reader_settings:delSetting(SEARCH_SETTING)
				G_reader_settings:delSetting(LANGUAGE_SETTING)
				G_reader_settings:delSetting(RATING_SETTING)
				if touchmenu_instance then
					touchmenu_instance:updateItems()
				end
			end,
		},
	}
	for _i, source in ipairs(available) do
		if filterItems(source, query, language, rating) then
			local label = source.name
			if isAidoku(source) then
				local details = {}
				if #(source.languages or {}) > 0 then
					if #source.languages <= 3 then
						details[#details + 1] = table.concat(source.languages, ",")
					else
						details[#details + 1] = T(_("%1 languages"), #source.languages)
					end
				end
				if source.content_rating ~= nil then
					details[#details + 1] = RATINGS[source.content_rating] or tostring(source.content_rating)
				end
				label = T(_("Aidoku: %1"), label)
				if #details > 0 then
					label = label .. " [" .. table.concat(details, " | ") .. "]"
				end
			end
			items[#items + 1] = {
				text = label,
				checked_func = function()
					return Sources.isSelected(plugin, source.id)
				end,
				keep_menu_open = true,
				callback = function(touchmenu_instance)
					Sources.toggle(plugin, source, touchmenu_instance)
				end,
			}
		end
	end
	if #items == 4 then
		items[#items + 1] = { text = _("No sources match these filters"), enabled = false }
	end
	return items
end

-- "Sources (1/3)" for the parent menu entry.
function Sources.summary(plugin)
	local available = Sources.available(plugin)
	local aidoku_count = 0
	for _i, source in ipairs(available) do
		if isAidoku(source) then
			aidoku_count = aidoku_count + 1
		end
	end
	return T(_("Sources (%1/%2, %3 Aidoku)"), #Sources.selected(plugin), #available, aidoku_count)
end

return Sources
