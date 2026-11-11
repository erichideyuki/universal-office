// Rotas da frente R da leva 5 do engine="rust" (comentários, revisões, campos/referências, localizar, ortografia, estilos, área de transferência sem evento, teclas Alt). Só dados, como engine-routes.ts (que junta esta tabela à
// principal): nenhum import do motor. Os handlers ficam em rust-engine-review.ts com o MESMO nome do método.
export const ROUTES_REVIEW = {
  /** métodos públicos atendidos pela sessão (handler em rust-engine-review.ts) */
  public: [] as readonly string[],
  /** métodos privados do elemento que o motor assume */
  private: [] as readonly string[],
  /** ids de comando das barras cujo run só chega a métodos roteados ou da casca */
  commands: [] as readonly string[],
  /** consultas desligadas que respondem o neutro sem rodar o código TS */
  neutral: [] as readonly string[],
};
