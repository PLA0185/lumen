import { useCallback, useEffect, useMemo, useState } from 'react'
import { Icon } from './Icons'
import { IpcError } from '../lib/ipc'
import { onDataChanged } from '../lib/data-change'
import * as api from '../lib/nutrition-ipc'
import type { EntryState, FoodCandidate, MealType, NutritionDashboard, NutritionEntry, Recipe, ShoppingDraft, ShoppingItem } from '../lib/nutrition-ipc'

const MEALS: Array<{ id: MealType; label: string }> = [
  { id: 'breakfast', label: '早餐' }, { id: 'lunch', label: '午餐' }, { id: 'dinner', label: '晚餐' }, { id: 'snack', label: '加餐' },
]
const SHOPPING_FILE_TYPES = '.pdf,.docx,.xlsx,.xls,.csv,.tsv,.txt,.md,.json,.xml,.html,.htm,.log,.png,.jpg,.jpeg,.webp,.gif'
type ImportRow = ShoppingDraft & { previewId: string; selected: boolean }

function todayLocal() {
  const date = new Date()
  return new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 10)
}
function errorText(error: unknown) { return error instanceof IpcError ? error.userMessage() : String(error) }
function asNumber(value: string) { const number = Number(value); return Number.isFinite(number) ? number : NaN }
function localId() { return crypto.randomUUID() }

export function NutritionView({ isAndroid = false }: { isAndroid?: boolean }) {
  const [date, setDate] = useState(todayLocal)
  const [dashboard, setDashboard] = useState<NutritionDashboard | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [targetInput, setTargetInput] = useState('')
  const [searchKeyConfigured, setSearchKeyConfigured] = useState(false)
  const [searchKey, setSearchKey] = useState('')
  const [foodQuery, setFoodQuery] = useState('')
  const [meal, setMeal] = useState<Exclude<MealType, 'training'>>('breakfast')
  const [foodGrams, setFoodGrams] = useState('100')
  const [foodState, setFoodState] = useState<EntryState>('eaten')
  const [candidates, setCandidates] = useState<FoodCandidate[]>([])
  const [selectedCandidateId, setSelectedCandidateId] = useState('')
  const [searching, setSearching] = useState(false)
  const [trainingName, setTrainingName] = useState('')
  const [trainingCalories, setTrainingCalories] = useState('')
  const [recipeTitle, setRecipeTitle] = useState('')
  const [recipeServings, setRecipeServings] = useState('1')
  const [recipeInstructions, setRecipeInstructions] = useState('')
  const [recipeIngredientGrams, setRecipeIngredientGrams] = useState('100')
  const [recipeIngredients, setRecipeIngredients] = useState<Array<{ candidate: FoodCandidate; grams: number }>>([])
  const [shoppingName, setShoppingName] = useState('')
  const [shoppingQuantity, setShoppingQuantity] = useState('1')
  const [shoppingUnit, setShoppingUnit] = useState('个')
  const [shoppingCategory, setShoppingCategory] = useState('其他')
  const [shoppingNotes, setShoppingNotes] = useState('')
  const [editingShopping, setEditingShopping] = useState<string | null>(null)
  const [editShoppingDraft, setEditShoppingDraft] = useState<ShoppingDraft | null>(null)
  const [editingEntryId, setEditingEntryId] = useState<string | null>(null)
  const [replacingEntryId, setReplacingEntryId] = useState<string | null>(null)
  const [editEntryGrams, setEditEntryGrams] = useState('')
  const [importMode, setImportMode] = useState<'local' | 'ai'>('local')
  const [importRows, setImportRows] = useState<ImportRow[]>([])
  const [importWarnings, setImportWarnings] = useState<string[]>([])
  const [importStatus, setImportStatus] = useState('')

  const reload = useCallback(async (forDate = date) => {
    setLoading(true)
    try {
      const next = await api.nutritionDashboard(forDate)
      setDashboard(next)
      setTargetInput(next.targetKcal?.toString() ?? '')
      try {
        setSearchKeyConfigured(await api.nutritionSearchKeyStatus())
        setError('')
      } catch (reason) {
        setSearchKeyConfigured(false)
        setError(errorText(reason))
      }
    } catch (reason) { setError(errorText(reason)) }
    finally { setLoading(false) }
  }, [date])

  useEffect(() => { void reload(date) }, [date, reload])
  useEffect(() => onDataChanged(['nutrition'], () => { void reload(date) }), [date, reload])

  const update = async (action: () => Promise<void>) => {
    setBusy(true); setError(''); setNotice('')
    try { await action() }
    catch (reason) { setError(errorText(reason)) }
    finally { setBusy(false) }
  }
  const refreshAfter = async (message?: string) => {
    await reload(date)
    if (message) setNotice(message)
  }

  const totals = dashboard?.totals
  const percent = totals && dashboard?.targetKcal
    ? Math.min(100, Math.max(0, Math.round(totals.netKcal / dashboard.targetKcal * 100)))
    : 0
  const openItems = useMemo(() => dashboard?.shoppingItems.filter(item => !item.completed) ?? [], [dashboard])
  const selectedCandidate = candidates.find(candidate => candidate.id === selectedCandidateId)
  const replacingEntry = dashboard?.entries.find(entry => entry.id === replacingEntryId)

  const saveTarget = () => update(async () => {
    const target = targetInput.trim() ? asNumber(targetInput) : null
    if (target !== null && (!Number.isInteger(target) || target < 1 || target > 100_000)) throw new Error('每日目标需为 1 到 100000 千卡的整数；清空后保存可移除目标。')
    await api.nutritionSetDailyTarget(target)
    await refreshAfter(target === null ? '每日目标已清除。' : '每日目标已保存。')
  })

  const lookup = async (event: React.FormEvent) => {
    event.preventDefault(); setSearching(true); setError(''); setCandidates([]); setSelectedCandidateId('')
    try {
      const result = await api.nutritionLookupFood(foodQuery)
      setCandidates(result)
      if (result[0]) setSelectedCandidateId(result[0].id)
      if (!result.length) setNotice('没有找到可核实的热量来源；请换成更具体的食物名称。')
    } catch (reason) { setError(errorText(reason)) }
    finally { setSearching(false) }
  }

  const addFood = () => update(async () => {
    if (!selectedCandidate) throw new Error('先联网查询食物热量并选择来源。')
    const grams = asNumber(foodGrams)
    if (!Number.isFinite(grams) || grams <= 0 || grams > 100_000) throw new Error('克数需大于 0 且不超过 100000。')
    if (replacingEntryId) {
      await api.nutritionEntryReplaceFood(replacingEntryId, selectedCandidate.id, grams, meal, foodState)
      setReplacingEntryId(null)
      await refreshAfter(`已将记录改为 ${selectedCandidate.name}，热量按新来源重新计算。`)
    } else {
      setDashboard(await api.nutritionEntryCreateFood(date, meal, selectedCandidate.id, grams, foodState))
      setNotice(`${selectedCandidate.name} 已${foodState === 'eaten' ? '记入实吃' : '加入计划'}；热量按联网来源重新计算。`)
    }
    setFoodQuery(''); setCandidates([]); setSelectedCandidateId('')
  })
  const beginReplaceFood = (entry: NutritionEntry) => {
    if (entry.mealType === 'training') return
    setReplacingEntryId(entry.id); setFoodQuery(entry.name); setMeal(entry.mealType)
    setFoodGrams(String(entry.amount)); setFoodState(entry.state); setCandidates([]); setSelectedCandidateId('')
    setError(''); setNotice('修改食物需要重新联网核对热量来源；原记录在确认保存前保持不变。')
  }

  const addIngredientDraft = () => {
    if (!selectedCandidate) { setError('先联网查询并选择配料的热量来源。'); return }
    const grams = asNumber(recipeIngredientGrams)
    if (!Number.isFinite(grams) || grams <= 0 || grams > 100_000) { setError('配料克数需大于 0 且不超过 100000。'); return }
    setRecipeIngredients(items => [...items, { candidate: selectedCandidate, grams }])
    setCandidates([]); setSelectedCandidateId(''); setFoodQuery('')
    setNotice(`已将 ${selectedCandidate.name} 加入食谱草稿。`)
  }

  const saveRecipe = () => update(async () => {
    const title = recipeTitle.trim(), servings = asNumber(recipeServings)
    if (!title || title.length > 120) throw new Error('请输入 1 到 120 个字的食谱名称。')
    if (!recipeIngredients.length) throw new Error('食谱至少需要一项经来源核实的配料。')
    const recipe = await api.nutritionRecipeCreate({
      title, servings, instructions: recipeInstructions,
      ingredients: recipeIngredients.map(item => ({ candidateId: item.candidate.id, amountGrams: item.grams })),
    })
    setRecipeTitle(''); setRecipeInstructions(''); setRecipeIngredients([])
    setDashboard(current => current ? { ...current, recipes: [recipe, ...current.recipes] } : current)
    setNotice(`已保存「${recipe.title}」，每份约 ${Math.round(recipe.caloriesPerServing)} 千卡。`)
  })

  const planRecipe = (recipe: Recipe, mealType: Exclude<MealType, 'training'>, servings = 1) => update(async () => {
    const next = await api.nutritionRecipePlan(date, recipe.id, mealType, servings)
    setDashboard(next); setNotice(`「${recipe.title}」已加入${MEALS.find(item => item.id === mealType)?.label}计划；标记实吃后才计入摄入。`)
  })
  const addRecipeShopping = (recipe: Recipe) => update(async () => {
    const items = await api.nutritionShoppingAddRecipe(recipe.id)
    setDashboard(current => current ? { ...current, shoppingItems: items } : current)
    setNotice(`已将「${recipe.title}」的配料合入采购清单。`)
  })

  const addTraining = () => update(async () => {
    const name = trainingName.trim(), calories = asNumber(trainingCalories)
    if (!name) throw new Error('请填写训练项目。')
    if (!Number.isInteger(calories) || calories <= 0 || calories > 100_000) throw new Error('训练消耗需为 1 到 100000 千卡的整数。')
    setDashboard(await api.nutritionEntryCreateTraining(date, name, calories))
    setTrainingName(''); setTrainingCalories(''); setNotice('训练消耗已单独记录，并从净摄入中扣除。')
  })

  const setEntryState = (entry: NutritionEntry) => update(async () => {
    await api.nutritionEntrySetState(entry.id, entry.state === 'eaten' ? 'planned' : 'eaten')
    await refreshAfter(entry.state === 'eaten' ? '已改回计划，热量不再计入今日摄入。' : '已标记实吃，并计入今日摄入。')
  })
  const changePortion = (entry: NutritionEntry) => {
    if (entry.mealType === 'training') return
    setEditingEntryId(entry.id); setEditEntryGrams(String(entry.amount))
  }
  const saveEntryPortion = (entry: NutritionEntry) => {
    void update(async () => {
      const amount = asNumber(editEntryGrams)
      if (!Number.isFinite(amount) || amount <= 0 || amount > 100_000) throw new Error('克数需大于 0 且不超过 100000。')
      await api.nutritionEntryUpdateAmount(entry.id, amount)
      setEditingEntryId(null)
      await refreshAfter('份量已调整，热量按记录来源重新计算。')
    })
  }
  const deleteEntry = (entry: NutritionEntry) => {
    if (!window.confirm(`移除「${entry.name}」这条记录？`)) return
    void update(async () => { await api.nutritionEntryDelete(entry.id); await refreshAfter('记录已移除。') })
  }

  const addShoppingItem = () => update(async () => {
    const draft = { name: shoppingName.trim(), quantity: asNumber(shoppingQuantity), unit: shoppingUnit.trim(), category: shoppingCategory.trim() || '其他', notes: shoppingNotes.trim() }
    if (!draft.name) throw new Error('请填写采购项目名称。')
    const item = await api.nutritionShoppingCreate(draft)
    setDashboard(current => current ? { ...current, shoppingItems: [...current.shoppingItems, item] } : current)
    setShoppingName(''); setShoppingQuantity('1'); setShoppingNotes(''); setNotice('采购项目已加入清单。')
  })
  const beginEditShopping = (item: ShoppingItem) => {
    setEditingShopping(item.id)
    setEditShoppingDraft({ name: item.name, quantity: item.quantity, unit: item.unit, category: item.category, notes: item.notes })
  }
  const saveShoppingEdit = (item: ShoppingItem) => update(async () => {
    if (!editShoppingDraft) return
    await api.nutritionShoppingUpdate(item.id, { ...editShoppingDraft, quantity: asNumber(String(editShoppingDraft.quantity)) })
    setEditingShopping(null); setEditShoppingDraft(null); await refreshAfter('采购项目已更新。')
  })
  const toggleShopping = (item: ShoppingItem, completed: boolean) => update(async () => {
    await api.nutritionShoppingSetCompleted(item.id, completed); await refreshAfter(completed ? '已标记为买好。' : '已放回待采购。')
  })
  const removeShopping = (item: ShoppingItem) => update(async () => {
    await api.nutritionShoppingDelete(item.id); await refreshAfter(`已移除「${item.name}」。`)
  })

  const previewShoppingFiles = async (files: FileList | null) => {
    if (!files?.length) return
    if (isAndroid && importMode === 'local' && Array.from(files).some(file => file.type.startsWith('image/'))) {
      setError('Android 本机模式不带图片 OCR。要导入图片或扫描件，请切换到“AI 整理”；文字类文件仍可本机识别。')
      setImportStatus('')
      return
    }
    setBusy(true); setError(''); setNotice(''); setImportRows([]); setImportWarnings([])
    const rows: ImportRow[] = [], warnings: string[] = []
    try {
      for (const file of Array.from(files)) {
        setImportStatus(`准备识别 ${file.name}…`)
        const preview = await api.nutritionShoppingImportPreview(file, importMode, progress => setImportStatus(`${progress.fileName}：${progress.message}`))
        rows.push(...preview.drafts.map(draft => ({ ...draft, previewId: localId(), selected: true })))
        warnings.push(...preview.warnings.map(warning => `${preview.fileName}：${warning}`))
      }
      setImportWarnings(warnings)
      if (!rows.length) {
        setImportStatus('没有识别到采购项目；可查看解析提示、切换 AI 整理，或手动添加。')
        return
      }
      setImportRows(rows); setImportWarnings(warnings); setImportStatus(`共识别 ${rows.length} 项，核对后再加入清单。`)
    } catch (reason) { setError(errorText(reason)); setImportStatus('') }
    finally { setBusy(false) }
  }
  const commitImport = () => update(async () => {
    const selected = importRows.filter(row => row.selected).map(({ previewId: _id, selected: _selected, ...draft }) => draft)
    if (!selected.length) throw new Error('请至少勾选一项采购内容。')
    await api.nutritionShoppingImportCommit(selected)
    const count = selected.length; setImportRows([]); setImportWarnings([]); setImportStatus('')
    await refreshAfter(`已确认导入 ${count} 项采购内容。`)
  })
  const saveSearchKey = () => update(async () => {
    if (!searchKey.trim()) throw new Error('请输入 Tavily Search API Key。')
    await api.nutritionSetSearchKey(searchKey.trim()); setSearchKey(''); setSearchKeyConfigured(true)
    setNotice('联网搜索密钥已保存在本机系统凭据库。')
  })
  const removeSearchKey = () => update(async () => {
    await api.nutritionSetSearchKey(''); setSearchKeyConfigured(false); setNotice('已删除本机联网搜索密钥。')
  })

  return (
    <div className="nutrition-view">
      <section className="nutrition-hero" aria-labelledby="nutrition-title">
        <div>
          <div className="nutrition-eyebrow">饮食与日常计划</div>
          <h2 id="nutrition-title">今天吃得怎么样</h2>
          <p>先查可核实的食物热量，再记录实吃或计划；训练消耗和加餐分开统计。</p>
        </div>
        <label className="nutrition-date">日期<input type="date" value={date} onChange={event => setDate(event.target.value)} /></label>
      </section>

      {error && <div className="nutrition-alert" role="alert"><span>{error}</span><button type="button" className="btn btn--ghost btn--sm" onClick={() => setError('')}>关闭</button></div>}
      {notice && <div className="nutrition-notice" role="status"><span>{notice}</span><button type="button" className="btn btn--ghost btn--sm" onClick={() => setNotice('')}>知道了</button></div>}
      {loading && !dashboard ? <div className="nutrition-loading" role="status">正在读取本机饮食记录…</div> : dashboard && totals ? <>
        <section className="nutrition-summary" aria-label="每日热量汇总">
          <div className="nutrition-summary__top">
            <div><span className="nutrition-eyebrow">净摄入</span><strong>{totals.netKcal.toLocaleString()} <small>千卡</small></strong></div>
            <label>每日目标（手动设置）<div className="nutrition-target"><input aria-label="每日热量目标" type="number" min="1" max="100000" step="1" value={targetInput} placeholder="未设置" onChange={event => setTargetInput(event.target.value)} /><span>千卡</span><button type="button" className="btn btn--primary btn--sm" disabled={busy} onClick={() => void saveTarget()}>保存</button></div></label>
          </div>
          {dashboard.targetKcal ? <div className="nutrition-progress" aria-label={`目标完成 ${percent}%`}><div style={{ width: `${percent}%` }} /></div> : <p className="nutrition-empty-target">设置目标后会显示当日进度；目标由你自行决定。</p>}
          <div className="nutrition-totals">
            <div><span>基础饮食</span><strong>{totals.baseKcal.toLocaleString()} <small>千卡</small></strong></div>
            <div><span>加餐</span><strong>{totals.snackKcal.toLocaleString()} <small>千卡</small></strong></div>
            <div><span>训练消耗</span><strong>−{totals.trainingKcal.toLocaleString()} <small>千卡</small></strong></div>
            <div className="nutrition-totals__remaining"><span>{totals.remainingKcal === null ? '尚未设置目标' : totals.remainingKcal >= 0 ? '目标剩余' : '超过目标'}</span><strong>{totals.remainingKcal === null ? '—' : Math.abs(totals.remainingKcal).toLocaleString()} <small>千卡</small></strong></div>
          </div>
          <p className="nutrition-footnote">只有标记“已吃”的食物计入摄入；净摄入 = 基础饮食 + 加餐 − 训练消耗。估算值用于个人记录，不代替专业营养建议。</p>
        </section>

        <div className="nutrition-layout">
          <div className="nutrition-column">
            <section className="nutrition-panel">
              <div className="nutrition-panel__heading"><div><span className="nutrition-eyebrow">联网核对</span><h3>添加食物</h3></div><span className={searchKeyConfigured ? 'nutrition-key-state is-ready' : 'nutrition-key-state'}>{searchKeyConfigured ? '搜索已配置' : '需要配置搜索密钥'}</span></div>
              {replacingEntry && <div className="nutrition-progress-note" role="status">正在修改「{replacingEntry.name}」：重新查询并选择热量来源后才会替换原记录。 <button type="button" className="btn btn--ghost btn--sm" onClick={() => { setReplacingEntryId(null); setCandidates([]); setSelectedCandidateId('') }}>取消修改</button></div>}
              <form className="nutrition-food-search" onSubmit={event => void lookup(event)}>
                <label>食物名称<input value={foodQuery} onChange={event => setFoodQuery(event.target.value)} placeholder="例如：水煮鸡胸肉、燕麦片" maxLength={160} /></label>
                <button type="submit" className="btn btn--primary" disabled={searching || !foodQuery.trim()}>{searching ? '联网查询中…' : <><Icon name="search" size={15} /> 查询热量</>}</button>
              </form>
              <p className="nutrition-muted">先联网查询食物热量并选择来源；候选热量必须带可打开的来源，没有可核对的候选时不能入账。</p>
              {searching && <div className="nutrition-progress-note" role="status">正在检索公开营养来源，再由 AI 按网页证据整理候选…</div>}
              {candidates.length > 0 && <div className="nutrition-candidates" role="radiogroup" aria-label="选择食物热量来源">
                <div className="nutrition-candidates__intro">选择实际匹配的食物；每个候选都必须有可打开的来源。</div>
                {candidates.map(candidate => <label key={candidate.id} className={`nutrition-candidate${candidate.id === selectedCandidateId ? ' is-selected' : ''}`}>
                  <input type="radio" name="nutrition-candidate" value={candidate.id} checked={candidate.id === selectedCandidateId} onChange={() => setSelectedCandidateId(candidate.id)} />
                  <span className="nutrition-candidate__body"><strong>{candidate.name}</strong><span>{candidate.kcalPer100g} 千卡 / 100 克</span><a href={candidate.sourceUrl} target="_blank" rel="noreferrer">{candidate.sourceTitle} ↗</a></span>
                </label>)}
                <div className="nutrition-entry-controls">
                  <label>餐次<select value={meal} onChange={event => setMeal(event.target.value as Exclude<MealType, 'training'>)}>{MEALS.map(item => <option key={item.id} value={item.id}>{item.label}</option>)}</select></label>
                  <label>食用量（克）<input type="number" min="0.1" max="100000" step="any" value={foodGrams} onChange={event => setFoodGrams(event.target.value)} /></label>
                  <label>状态<select value={foodState} onChange={event => setFoodState(event.target.value as EntryState)}><option value="eaten">已吃（计入摄入）</option><option value="planned">计划（暂不计入）</option></select></label>
                  <button type="button" className="btn btn--primary" disabled={busy || !selectedCandidate} onClick={() => void addFood()}>{replacingEntryId ? '保存修改' : '记录食物'}</button>
                  <label>食谱配料克数<input type="number" min="0.1" step="any" value={recipeIngredientGrams} onChange={event => setRecipeIngredientGrams(event.target.value)} /></label>
                  <button type="button" className="btn btn--ghost" disabled={!selectedCandidate} onClick={addIngredientDraft}>加入食谱草稿</button>
                </div>
              </div>}
              <details className="nutrition-disclosure">
                <summary>联网来源设置与隐私说明</summary>
                <p>食物名称会发给 Tavily 搜索；搜索摘录和食物名称会发给你在“设置 → AI”里配置的模型核对。保存的记录会保留来源标题与链接。密钥只存本机凭据库。</p>
                <p><a href="https://app.tavily.com/home" target="_blank" rel="noreferrer">打开 Tavily 获取 Search API Key ↗</a> · <a href="https://www.tavily.com/pricing" target="_blank" rel="noreferrer">查看官方额度与定价 ↗</a></p>
                <div className="nutrition-key-form"><input type="password" value={searchKey} autoComplete="new-password" aria-label="Tavily Search API Key" placeholder={searchKeyConfigured ? '已配置；输入新密钥可替换' : '粘贴 Tavily Search API Key'} onChange={event => setSearchKey(event.target.value)} /><button type="button" className="btn btn--primary btn--sm" disabled={busy || !searchKey.trim()} onClick={() => void saveSearchKey()}>保存密钥</button>{searchKeyConfigured && <button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void removeSearchKey()}>删除本机密钥</button>}</div>
              </details>
            </section>

            <section className="nutrition-panel">
              <div className="nutrition-panel__heading"><div><span className="nutrition-eyebrow">已核实配料</span><h3>我的食谱</h3></div><span>{dashboard.recipes.length} 份</span></div>
              <div className="nutrition-recipe-form">
                <label>食谱名称<input value={recipeTitle} onChange={event => setRecipeTitle(event.target.value)} maxLength={120} placeholder="例如：鸡胸肉蔬菜饭" /></label>
                <label>整份包含几份<input type="number" min="0.1" max="1000" step="any" value={recipeServings} onChange={event => setRecipeServings(event.target.value)} /></label>
                <label className="nutrition-recipe-form__instructions">做法（可选）<textarea value={recipeInstructions} onChange={event => setRecipeInstructions(event.target.value)} maxLength={10000} rows={2} placeholder="记录简要做法" /></label>
                {recipeIngredients.length > 0 && <ul className="nutrition-draft-ingredients">{recipeIngredients.map((ingredient, index) => <li key={`${ingredient.candidate.id}-${index}`}><span>{ingredient.candidate.name} · {ingredient.grams} 克 · 约 {Math.round(ingredient.grams * ingredient.candidate.kcalPer100g / 100)} 千卡</span><button type="button" aria-label={`移除 ${ingredient.candidate.name}`} onClick={() => setRecipeIngredients(items => items.filter((_, i) => i !== index))}>移除</button></li>)}</ul>}
                <button type="button" className="btn btn--primary" disabled={busy || !recipeTitle.trim() || recipeIngredients.length === 0} onClick={() => void saveRecipe()}>保存食谱</button>
                <p className="nutrition-muted">先在上面的“添加食物”中联网查配料，再点“加入食谱草稿”。每份热量由来源数据和克数自动计算。</p>
              </div>
              {dashboard.recipes.length === 0 ? <p className="nutrition-empty">还没有食谱；配料热量必须先经联网来源核对。</p> : <div className="nutrition-recipe-list">{dashboard.recipes.map(recipe => <article className="nutrition-recipe" id={`recipe-${recipe.id}`} key={recipe.id}>
                <div className="nutrition-recipe__top"><div><h4>{recipe.title}</h4><strong>约 {Math.round(recipe.caloriesPerServing)} 千卡 / 份</strong></div><span>{recipe.ingredients.length} 项配料</span></div>
                {recipe.instructions && <p>{recipe.instructions}</p>}
                <ul>{recipe.ingredients.map(ingredient => <li key={ingredient.id}><span>{ingredient.foodName} · {ingredient.amountGrams} 克</span><a href={ingredient.sourceUrl} target="_blank" rel="noreferrer">来源 ↗</a></li>)}</ul>
                <div className="nutrition-recipe__actions"><button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void planRecipe(recipe, 'breakfast')}>安排早餐</button><button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void planRecipe(recipe, 'lunch')}>安排午餐</button><button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void planRecipe(recipe, 'dinner')}>安排晚餐</button><button type="button" className="btn btn--primary btn--sm" disabled={busy} onClick={() => void addRecipeShopping(recipe)}>配料加入采购</button><button type="button" className="btn btn--danger btn--sm" disabled={busy} onClick={() => { if (window.confirm(`删除「${recipe.title}」？已有饮食记录会保留。`)) void update(async () => { await api.nutritionRecipeDelete(recipe.id); await refreshAfter('食谱已删除，已记录的摄入仍保留。') }) }}>删除</button></div>
              </article>)}</div>}
            </section>

            <section className="nutrition-panel">
              <div className="nutrition-panel__heading"><div><span className="nutrition-eyebrow">单独扣除</span><h3>训练消耗</h3></div></div>
              <form className="nutrition-training-form" onSubmit={event => { event.preventDefault(); void addTraining() }}>
                <label>训练内容<input value={trainingName} onChange={event => setTrainingName(event.target.value)} placeholder="例如：跑步 30 分钟" maxLength={120} /></label>
                <label>消耗（千卡）<input type="number" min="1" max="100000" step="1" value={trainingCalories} onChange={event => setTrainingCalories(event.target.value)} placeholder="按运动设备或可靠估算填写" /></label>
                <button type="submit" className="btn btn--primary" disabled={busy}>记录训练</button>
              </form>
            </section>
          </div>

          <div className="nutrition-column">
            <section className="nutrition-panel">
              <div className="nutrition-panel__heading"><div><span className="nutrition-eyebrow">按餐次查看</span><h3>今日记录</h3></div><span>{dashboard.entries.length} 条</span></div>
              {dashboard.entries.length === 0 ? <p className="nutrition-empty">还没有记录。计划中的食物不会计入今日热量。</p> : <div className="nutrition-entry-list">{dashboard.entries.map(entry => <article className={`nutrition-entry${entry.state === 'planned' ? ' is-planned' : ''}${entry.mealType === 'training' ? ' is-training' : ''}`} key={entry.id}>
                <div className="nutrition-entry__main"><div><span className="nutrition-entry__meal">{entry.mealType === 'training' ? '训练消耗' : MEALS.find(item => item.id === entry.mealType)?.label ?? '饮食'}</span><strong>{entry.name}</strong></div><div className="nutrition-entry__calories">{entry.mealType === 'training' ? '−' : ''}{entry.calories.toLocaleString()} <small>千卡</small></div></div>
                <div className="nutrition-entry__meta"><span>{entry.amount} {entry.unit}{entry.mealType !== 'training' ? ` · ${entry.kcalPerUnit} 千卡 / ${entry.unit === 'g' ? '100 克' : '份'}` : ''}</span><span className={entry.state === 'eaten' ? 'nutrition-entry__status is-eaten' : 'nutrition-entry__status'}>{entry.state === 'eaten' ? '已吃' : '计划'}</span></div>
                {entry.sourceUrl && <a className="nutrition-entry__source" href={entry.sourceUrl} target="_blank" rel="noreferrer">热量来源：{entry.sourceTitle ?? '打开来源'} ↗</a>}
                {entry.recipeId && <a className="nutrition-entry__source" href={`#recipe-${entry.recipeId}`}>热量依据：食谱「{entry.sourceTitle?.replace(/^食谱：/, '') ?? entry.name}」中的联网核实配料</a>}
                {editingEntryId === entry.id ? <div className="nutrition-entry__edit"><label>调整份量（{entry.unit}）<input type="number" min="0.1" max="100000" step="any" value={editEntryGrams} onChange={event => setEditEntryGrams(event.target.value)} /></label><button type="button" className="btn btn--primary btn--sm" disabled={busy} onClick={() => saveEntryPortion(entry)}>保存</button><button type="button" className="btn btn--ghost btn--sm" onClick={() => setEditingEntryId(null)}>取消</button></div> : <div className="nutrition-entry__actions">{entry.mealType !== 'training' && <><button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => changePortion(entry)}>改份量</button><button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => beginReplaceFood(entry)}>改食物</button><button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => setEntryState(entry)}>{entry.state === 'eaten' ? '改为计划' : '标记已吃'}</button></>}<button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => deleteEntry(entry)}>移除</button></div>}
              </article>)}</div>}
            </section>

            <section className="nutrition-panel nutrition-shopping">
              <div className="nutrition-panel__heading"><div><span className="nutrition-eyebrow">像待办一样管理</span><h3>采购清单</h3></div><span>{openItems.length} 项待买</span></div>
              <form className="nutrition-shopping-form" onSubmit={event => { event.preventDefault(); void addShoppingItem() }}>
                <label>要买什么<input value={shoppingName} onChange={event => setShoppingName(event.target.value)} placeholder="商品或食材" maxLength={120} /></label>
                <label>数量<input type="number" min="0.01" max="100000" step="any" value={shoppingQuantity} onChange={event => setShoppingQuantity(event.target.value)} /></label>
                <label>单位<input value={shoppingUnit} onChange={event => setShoppingUnit(event.target.value)} maxLength={24} /></label>
                <label>分类<input value={shoppingCategory} onChange={event => setShoppingCategory(event.target.value)} maxLength={40} /></label>
                <button type="submit" className="btn btn--primary" disabled={busy || !shoppingName.trim()}><Icon name="plus" size={15} /> 添加</button>
              </form>
              <div className="nutrition-import">
                <div className="nutrition-import__heading"><strong>从文件导入</strong><span>先识别成预览，确认后才写入</span></div>
                <div className="nutrition-import__controls"><label>识别方式<select value={importMode} onChange={event => setImportMode(event.target.value as 'local' | 'ai')}><option value="local">{isAndroid ? '本机识别（文字类文件）' : '本机识别'}</option><option value="ai">AI 整理（文字和扫描图会发送给模型）</option></select></label><label className="btn btn--ghost nutrition-file-button"><Icon name="upload" size={15} />选择文件<input type="file" accept={SHOPPING_FILE_TYPES} multiple disabled={busy} onChange={event => { void previewShoppingFiles(event.target.files); event.currentTarget.value = '' }} /></label></div>
                <p>{isAndroid ? '支持 PDF、DOCX、XLSX/XLS、图片、CSV/TSV、JSON、HTML/XML、Markdown、日志和文本；单个文件最多 20 MiB。Android 本机模式可提取文字，但扫描图片/PDF 和内嵌图片需要切换 AI 整理。AI 模式会向设置中的 AI 服务商发送提取文字及需要识别的扫描页或内嵌图。原件不会自动保存到资料库。' : '支持 PDF、DOCX、XLSX/XLS、图片、CSV/TSV、JSON、HTML/XML、Markdown、日志和文本；单个文件最多 20 MiB。本机模式在本机提取，图片 OCR 使用 PaddleOCR；AI 模式会向设置中的 AI 服务商发送提取文字及需要识别的扫描页或内嵌图。原件不会自动保存到资料库。'}</p>
                {importStatus && <div role="status" className="nutrition-progress-note">{importStatus}</div>}
              </div>
              {importWarnings.length > 0 && <details className="nutrition-import-warnings"><summary>解析提示（{importWarnings.length}）</summary>{importWarnings.map((warning, index) => <p key={index}>{warning}</p>)}</details>}
              {importRows.length > 0 && <div className="nutrition-import-preview">
                <div className="nutrition-import-preview__top"><strong>导入预览 · {importRows.length} 项</strong><button type="button" className="btn btn--ghost btn--sm" onClick={() => setImportRows(rows => rows.map(row => ({ ...row, selected: !rows.every(item => item.selected) })))}>{importRows.every(row => row.selected) ? '取消全选' : '全选'}</button></div>
                <div className="nutrition-import-preview__rows">{importRows.map(row => <article key={row.previewId} className="nutrition-import-row">
                  <label className="nutrition-import-row__select"><input type="checkbox" checked={row.selected} onChange={event => setImportRows(rows => rows.map(item => item.previewId === row.previewId ? { ...item, selected: event.target.checked } : item))} /><input aria-label="采购名称" value={row.name} onChange={event => setImportRows(rows => rows.map(item => item.previewId === row.previewId ? { ...item, name: event.target.value } : item))} /></label>
                  <input aria-label="采购数量" type="number" min="0.01" step="any" value={row.quantity} onChange={event => setImportRows(rows => rows.map(item => item.previewId === row.previewId ? { ...item, quantity: asNumber(event.target.value) } : item))} />
                  <input aria-label="采购单位" value={row.unit} onChange={event => setImportRows(rows => rows.map(item => item.previewId === row.previewId ? { ...item, unit: event.target.value } : item))} />
                  <input aria-label="采购分类" value={row.category} onChange={event => setImportRows(rows => rows.map(item => item.previewId === row.previewId ? { ...item, category: event.target.value } : item))} />
                  <button type="button" className="btn btn--ghost btn--sm" aria-label={`移除 ${row.name}`} onClick={() => setImportRows(rows => rows.filter(item => item.previewId !== row.previewId))}>移除</button>
                </article>)}</div>
                <div className="nutrition-import-preview__actions"><button type="button" className="btn btn--primary" disabled={busy || !importRows.some(row => row.selected)} onClick={() => void commitImport()}>确认导入已选项目</button><button type="button" className="btn btn--ghost" onClick={() => { setImportRows([]); setImportWarnings([]); setImportStatus('') }}>取消</button></div>
              </div>}
              {dashboard.shoppingItems.length === 0 ? <p className="nutrition-empty">采购清单还是空的；也可以从食谱配料一键生成。</p> : <div className="nutrition-shopping-list">{dashboard.shoppingItems.map(item => <article className={`nutrition-shopping-item${item.completed ? ' is-completed' : ''}`} key={item.id}>
                <input type="checkbox" aria-label={`${item.completed ? '放回待买' : '标记买好'}：${item.name}`} checked={item.completed} onChange={event => void toggleShopping(item, event.target.checked)} />
                <div className="nutrition-shopping-item__content">{editingShopping === item.id && editShoppingDraft ? <div className="nutrition-shopping-edit">
                  <input aria-label="编辑名称" value={editShoppingDraft.name} onChange={event => setEditShoppingDraft(draft => draft && ({ ...draft, name: event.target.value }))} />
                  <input aria-label="编辑数量" type="number" min="0.01" step="any" value={editShoppingDraft.quantity} onChange={event => setEditShoppingDraft(draft => draft && ({ ...draft, quantity: asNumber(event.target.value) }))} />
                  <input aria-label="编辑单位" value={editShoppingDraft.unit} onChange={event => setEditShoppingDraft(draft => draft && ({ ...draft, unit: event.target.value }))} />
                  <input aria-label="编辑分类" value={editShoppingDraft.category} onChange={event => setEditShoppingDraft(draft => draft && ({ ...draft, category: event.target.value }))} />
                  <div><button type="button" className="btn btn--primary btn--sm" disabled={busy} onClick={() => void saveShoppingEdit(item)}>保存</button><button type="button" className="btn btn--ghost btn--sm" onClick={() => setEditingShopping(null)}>取消</button></div>
                </div> : <><strong>{item.name}</strong><span>{item.quantity} {item.unit} · {item.category}</span>{item.notes && <small>{item.notes}</small>}</>}</div>
                {editingShopping !== item.id && <div className="nutrition-shopping-item__actions"><button type="button" className="btn btn--ghost btn--sm" onClick={() => beginEditShopping(item)}>编辑</button><button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void removeShopping(item)}>删除</button></div>}
              </article>)}</div>}
            </section>
          </div>
        </div>
      </> : null}
    </div>
  )
}
