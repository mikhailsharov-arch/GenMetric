import { useLayoutEffect, type RefObject } from "react";

/**
 * Список «Набрано» занимает свободное место до низа окна (Роман 19.10.2026:
 * «Если на экране есть место, пусть программа растягивает список
 * предыдущих записей до самого низа окна»). Высота из стилей (14em) —
 * наименьшая; место считается при показе формы, смене размера окна и списка.
 */
export function useFillDown(ref: RefObject<HTMLElement | null>, deps: unknown[]): void {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      if (el.offsetParent === null) return;
      el.style.maxHeight = "";
      const least = parseFloat(getComputedStyle(el).maxHeight) || 0;
      const room = window.innerHeight - el.getBoundingClientRect().top - 12;
      if (room > least) el.style.maxHeight = `${Math.floor(room)}px`;
    };
    fit();
    const seen = new IntersectionObserver(fit);
    seen.observe(el);
    // Блоки над списком свернули или развернули, сменили масштаб — место
    // другое (ревьюер 19.10.2026).
    const sized = new ResizeObserver(() => fit());
    if (el.parentElement) sized.observe(el.parentElement.parentElement ?? el.parentElement);
    window.addEventListener("resize", fit);
    return () => { seen.disconnect(); sized.disconnect(); window.removeEventListener("resize", fit); };
  }, deps);
}
